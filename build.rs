//! Build script for janq.
//!
//! Windows: embeds the application icon resource into the executable via janq.rc.

fn main() {
  let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

  if target == "windows" {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon-b.ico");
    println!("cargo:rerun-if-changed=assets/icon-w.ico");
    println!("cargo:rerun-if-changed=assets/janq.rc");
    let _ = embed_resource::compile("assets/janq.rc", embed_resource::NONE);
  }
}
