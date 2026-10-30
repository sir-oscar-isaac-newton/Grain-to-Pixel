//! Build script. On Windows (MSVC) the Tauri runtime needs Common Controls v6, which comes from an
//! application manifest. `tauri-build` only embeds its manifest in the main binary, so unit tests and
//! examples (the IPC generator) die at start with STATUS_ENTRYPOINT_NOT_FOUND. We therefore embed our
//! own manifest in every target and tell `tauri-build` not to add a second one.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let is_windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");

    let mut attributes = tauri_build::Attributes::new();
    if is_windows_msvc {
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
    }
    tauri_build::try_build(attributes)?;

    if is_windows_msvc {
        let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?)
            .join("windows-app-manifest.xml");
        println!("cargo::rerun-if-changed=windows-app-manifest.xml");
        println!("cargo::rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo::rustc-link-arg=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
    Ok(())
}
