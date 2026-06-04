//! Minimal dbusmenu implementation for KDE Plasma systray context menu.
//!
//! Replaces ksni with a hand-rolled `com.canonical.dbusmenu` served on the
//! existing zbus connection. Menu items are built from the current config on
//! each `GetLayout` call.

use std::collections::HashMap;
use std::process::exit;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{interface, Connection};

use crate::linux::hotkey::normalize_shortcut_for_kde;
use crate::linux::kwin::{recover_all, restore_quake, toggle_quake};
use janq::config::Config;
use janq::shutdown::{print_shutdown_message, print_termination_complete};

/// Dbusmenu service providing a right-click context menu for the tray icon.
pub struct DbusmenuService {
  pub config: Arc<RwLock<Config>>,
  pub conn: Connection,
  pub revision: AtomicU32,
}

// Menu ID scheme:
//   0       = root
//   1..N    = app entries (config order)
//   10000   = separator (stable across reloads)
//   10001   = "Quit" (stable across reloads)

/// Build properties map for a single menu item.
fn item_props(
  label: &str,
  item_type: Option<&str>,
  shortcut: Option<Vec<String>>,
) -> HashMap<String, OwnedValue> {
  let mut m = HashMap::new();
  if let Some(t) = item_type {
    m.insert("type".into(), Value::from(t).try_into().unwrap());
  }
  if !label.is_empty() {
    m.insert("label".into(), Value::from(label).try_into().unwrap());
  }
  if let Some(parts) = shortcut {
    // Dbusmenu shortcut type: aas (array of array of string)
    let shortcut_val: Vec<Vec<String>> = vec![parts];
    m.insert(
      "shortcut".into(),
      Value::from(shortcut_val).try_into().unwrap(),
    );
  }
  m
}

/// Build the full menu layout tuple from current config.
fn build_layout(config: &Config) -> (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>) {
  let mut children: Vec<OwnedValue> = Vec::new();

  for (i, (name, app_cfg)) in config.app.iter().enumerate() {
    let id = (i + 1) as i32;

    // Build shortcut display (NBSP padding trick matching old ksni behavior)
    let hotkeys = app_cfg.hotkey.as_vec();
    let (shortcut_parts, normalized) = if !hotkeys.is_empty() {
      let n = normalize_shortcut_for_kde(&hotkeys[0]);
      let parts: Vec<String> = n.split('+').map(|s| s.trim().to_string()).collect();
      (Some(parts), n)
    } else {
      (None, String::new())
    };

    let name_len = name.chars().count();
    let shortcut_len = normalized.chars().count();
    let padding = 20usize.saturating_sub(name_len + shortcut_len).max(5);
    let label = format!("{}{}", name, "\u{00A0}".repeat(padding));

    let props = item_props(&label, None, shortcut_parts);
    let child: (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>) = (id, props, vec![]);
    children.push(Value::from(child).try_into().unwrap());
  }

  // Separator
  {
    let props = item_props("", Some("separator"), None);
    let child: (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>) = (10000, props, vec![]);
    children.push(Value::from(child).try_into().unwrap());
  }

  // Recover
  {
    let props = item_props("Recover", None, None);
    let child: (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>) = (10002, props, vec![]);
    children.push(Value::from(child).try_into().unwrap());
  }

  // Quit
  {
    let props = item_props("Quit", None, None);
    let child: (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>) = (10001, props, vec![]);
    children.push(Value::from(child).try_into().unwrap());
  }

  // Root — children-display tells KDE this is a submenu (prevents bold rendering)
  let mut root_props: HashMap<String, OwnedValue> = HashMap::new();
  root_props.insert(
    "children-display".into(),
    Value::from("submenu").try_into().unwrap(),
  );
  (0, root_props, children)
}

#[interface(name = "com.canonical.dbusmenu")]
impl DbusmenuService {
  fn get_layout(
    &self,
    parent_id: i32,
    _recursion_depth: i32,
    _property_names: Vec<String>,
  ) -> zbus::fdo::Result<(u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>))> {
    let rev = self.revision.load(Ordering::Relaxed);
    if parent_id != 0 {
      // Only root layout supported
      let empty: (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>) =
        (parent_id, HashMap::new(), vec![]);
      return Ok((rev, empty));
    }
    let config = self.config.read().unwrap().clone();
    let layout = build_layout(&config);
    Ok((rev, layout))
  }

  fn event(
    &self,
    id: i32,
    event_id: &str,
    _data: OwnedValue,
    _timestamp: u32,
  ) -> zbus::fdo::Result<()> {
    if event_id != "clicked" {
      return Ok(());
    }
    let config = self.config.read().unwrap().clone();
    let app_count = config.app.len() as i32;

    if id == 10001 {
      let conn = self.conn.clone();
      tokio::spawn(async move {
        print_shutdown_message("Quit via systray");
        let _ = restore_quake(&config, &conn).await;
        // Ensure KWin scripts have time to finish before process exit
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        print_termination_complete();
        exit(0);
      });
    } else if id == 10002 {
      let config = config.clone();
      let conn = self.conn.clone();
      tokio::spawn(async move {
        recover_all(&config, &conn).await;
      });
    } else if id >= 1 && id <= app_count {
      let idx = (id - 1) as usize;
      if let Some(name) = config.app.keys().nth(idx) {
        let name = name.clone();
        let config = config.clone();
        let conn = self.conn.clone();
        tokio::spawn(async move {
          let _ = toggle_quake(&name, &config, &conn).await;
        });
      }
    }
    Ok(())
  }

  fn about_to_show(&self, _id: i32) -> zbus::fdo::Result<bool> {
    Ok(false)
  }

  fn get_group_properties(
    &self,
    ids: Vec<i32>,
    _property_names: Vec<String>,
  ) -> zbus::fdo::Result<Vec<(i32, HashMap<String, OwnedValue>)>> {
    let config = self.config.read().unwrap().clone();
    let app_count = config.app.len() as i32;
    let mut result = Vec::new();

    for id in ids {
      if id == 0 {
        result.push((0, HashMap::new()));
      } else if id >= 1 && id <= app_count {
        let idx = (id - 1) as usize;
        if let Some((name, app_cfg)) = config.app.iter().nth(idx) {
          let hotkeys = app_cfg.hotkey.as_vec();
          let (shortcut_parts, normalized) = if !hotkeys.is_empty() {
            let n = normalize_shortcut_for_kde(&hotkeys[0]);
            let parts: Vec<String> = n.split('+').map(|s| s.trim().to_string()).collect();
            (Some(parts), n)
          } else {
            (None, String::new())
          };
          let name_len = name.chars().count();
          let shortcut_len = normalized.chars().count();
          let padding = 20usize.saturating_sub(name_len + shortcut_len).max(5);
          let label = format!("{}{}", name, "\u{00A0}".repeat(padding));
          result.push((id, item_props(&label, None, shortcut_parts)));
        }
      } else if id == 10000 {
        result.push((id, item_props("", Some("separator"), None)));
      } else if id == 10001 {
        result.push((id, item_props("Quit", None, None)));
      } else if id == 10002 {
        result.push((id, item_props("Recover", None, None)));
      }
    }
    Ok(result)
  }

  #[zbus(property)]
  fn version(&self) -> u32 {
    3
  }

  #[zbus(property)]
  fn text_direction(&self) -> String {
    "ltr".into()
  }

  #[zbus(property)]
  fn status(&self) -> String {
    "normal".into()
  }

  #[zbus(signal)]
  async fn layout_updated(
    signal_ctxt: &SignalEmitter<'_>,
    revision: u32,
    parent: i32,
  ) -> zbus::Result<()>;

  #[zbus(signal)]
  async fn items_properties_updated(
    signal_ctxt: &SignalEmitter<'_>,
    updated_props: &[(i32, HashMap<String, OwnedValue>)],
    removed_props: &[(i32, Vec<String>)],
  ) -> zbus::Result<()>;
}

impl DbusmenuService {
  /// Notify KDE that the menu layout has changed (call after config reload).
  pub async fn notify_layout_changed(conn: &Connection) {
    let rev = {
      let iface_ref = conn
        .object_server()
        .interface::<_, DbusmenuService>("/MenuBar")
        .await;
      if let Ok(iface) = iface_ref {
        let svc = iface.get().await;
        svc.revision.fetch_add(1, Ordering::Relaxed) + 1
      } else {
        return;
      }
    };
    // Emit signal outside the interface lock
    let iface_ref = conn
      .object_server()
      .interface::<_, DbusmenuService>("/MenuBar")
      .await;
    if let Ok(iface) = iface_ref {
      let ctxt = iface.signal_emitter();
      let _ = DbusmenuService::layout_updated(&ctxt, rev, 0).await;
    }
  }
}
