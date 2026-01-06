use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
  let target = env::var("TARGET").unwrap();
  if target.contains("windows") {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let icon_path = Path::new(&manifest_dir).join("icon.ico");

    // Always tell cargo to rebuild if the icon changes
    println!("cargo:rerun-if-changed={}", icon_path.display());

    if target.contains("gnu") {
      let out_dir = env::var("OUT_DIR").unwrap();
      let rc_path = Path::new(&out_dir).join("ruake_generated.rc");
      let obj_path = Path::new(&out_dir).join("ruake_icon.o");

      // Generate .rc with absolute path to icon to avoid path resolution issues in windres
      let rc_content = format!("id ICON \"{}\"", icon_path.display().to_string().replace("\\", "/"));
      std::fs::write(&rc_path, rc_content).unwrap();

      let windres_path = "/usr/bin/x86_64-w64-mingw32-windres";

      if Path::new(windres_path).exists() {
        let status = Command::new(windres_path)
          .arg("-i")
          .arg(&rc_path)
          .arg("-o")
          .arg(&obj_path)
          .status()
          .expect("Failed to start windres");

        if status.success() {
          println!("cargo:rustc-link-arg={}", obj_path.display());
        }
      }
    } else {
      let mut res = winres::WindowsResource::new();
      res.set_icon(icon_path.to_str().unwrap());
      let _ = res.compile();
    }
  }
}
