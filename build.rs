//! Build script for janq.
//!
//! - Windows: embeds the application icon resource into the executable via janq.rc.
//! - Linux: pre-renders icon-symbolic.svg to ARGB pixmaps at tray sizes so the SNI
//!   daemon can serve them directly via IconPixmap when `mono_icon = true`. The
//!   colored variant is served via IconName and resolved by Plasma from the hicolor
//!   theme, so no colored ARGB is embedded. Doing this at build time keeps resvg
//!   out of the runtime binary and avoids SVG parsing at startup.
//!
//! `embed-resource` handles Windows resource compilation; `resvg` is used by this
//! script only and is never linked into the final runtime binary.

fn main() {
  let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

  if target == "windows" {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon-b.ico");
    println!("cargo:rerun-if-changed=assets/icon-w.ico");
    println!("cargo:rerun-if-changed=assets/janq.rc");
    let _ = embed_resource::compile("assets/janq.rc", embed_resource::NONE);
  }

  if target == "linux" {
    println!("cargo:rerun-if-changed=assets/icon-symbolic.svg");
    render_symbolic_tray_pixmaps();
  }
}

fn render_symbolic_tray_pixmaps() {
  use resvg::tiny_skia::{Pixmap, Transform};
  use resvg::usvg::{Options, Tree};
  use std::fs;
  use std::path::PathBuf;

  let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
  let sizes: [u32; 4] = [22, 32, 48, 64];

  // The symbolic SVG uses fill/stroke="currentColor" which resolves to black under
  // a default CSS context. Force white so the demultiplied output carries the
  // silhouette entirely in the alpha channel — runtime can then swap RGB to any
  // theme color without touching alpha.
  let symbolic_src =
    fs::read_to_string("assets/icon-symbolic.svg").expect("read icon-symbolic.svg");
  let symbolic = symbolic_src.replace("currentColor", "#ffffff");

  let opt = Options::default();
  let tree = Tree::from_data(symbolic.as_bytes(), &opt).expect("parse icon-symbolic.svg");
  let svg_size = tree.size();
  let max_dim = svg_size.width().max(svg_size.height());

  for &size in &sizes {
    let scale = size as f32 / max_dim;
    let mut pixmap = Pixmap::new(size, size).expect("pixmap alloc");
    resvg::render(
      &tree,
      Transform::from_scale(scale, scale),
      &mut pixmap.as_mut(),
    );

    // Convert tiny-skia's premultiplied RGBA to ARGB non-premultiplied (network byte
    // order per the SNI spec). Plasma handles both, but non-premultiplied is safer
    // across other StatusNotifierHost implementations.
    let src = pixmap.data();
    let mut alpha_only = Vec::with_capacity(src.len() / 4);
    for chunk in src.chunks_exact(4) {
      alpha_only.push(chunk[3]); // Only need Alpha
    }

    let path = PathBuf::from(&out_dir).join(format!("symbolic_{}.alpha", size));
    fs::write(&path, &alpha_only).unwrap_or_else(|e| panic!("write {:?}: {e}", path));
  }
}
