//! Build script for janq.
//!
//! On Windows targets, this script embeds the application icon into the executable.
//! The icon is defined in `janq.rc` (a Windows resource script) which references `icon.ico`.
//! The `embed-resource` crate handles compiling the resource script and linking it into
//! the final PE executable, ensuring the icon appears in Windows Explorer and the taskbar.
//!
//! This approach works with both MSVC and GNU (MinGW) toolchains, unlike the previous
//! manual windres approach which had linking order issues with `cargo:rustc-link-arg`.

fn main() {
  // Only run resource compilation when targeting Windows
  if std::env::var("TARGET")
    .map(|t| t.contains("windows"))
    .unwrap_or(false)
  {
    // Rebuild if the icon or resource script changes
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-changed=janq.rc");

    // Compile janq.rc -> object file and link it into the executable.
    // This embeds the icon at resource ID 1 (the default application icon).
    let _ = embed_resource::compile("janq.rc", embed_resource::NONE);
  }
}
