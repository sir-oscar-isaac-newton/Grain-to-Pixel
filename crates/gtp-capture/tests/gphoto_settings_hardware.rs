//! Needs a Nikon D600 (or another gphoto2 camera) on USB, and macOS with libgphoto2 (`brew install
//! libgphoto2 pkgconf`), so it is ignored and built only with the `tether` feature on macOS. It does
//! not touch the Scanlight. Run it on the Mac with
//!
//! ```text
//! cargo test -p gtp-capture --features tether --test gphoto_settings_hardware -- --ignored --nocapture
//! ```
//!
//! It prints the ISO, shutter speed and aperture with their value, whether they can be changed and
//! the choices the camera gives. With `GTP_GPHOTO_SET_ISO=<value>` (one of the printed ISO
//! choices) it then sets the ISO and prints the settings again. The camera must be switched on and
//! free of macOS's own camera helper (log other accounts out, quit Photos and Image Capture).
#![cfg(all(feature = "tether", target_os = "macos"))]

use gtp_capture::{
    configure_library_paths, CameraSetting, CameraSettingKind, CameraSettings, GphotoCamera,
};

fn print(title: &str, settings: &[CameraSetting]) {
    println!("{title}");
    for setting in settings {
        println!(
            "  {:?} (key {}): value {:?}, writable {}, {} choices: {:?}",
            setting.kind,
            setting.kind.key(),
            setting.value,
            setting.writable,
            setting.choices.len(),
            setting.choices
        );
    }
}

#[test]
#[ignore = "needs a Nikon D600 on USB"]
fn the_three_exposure_settings_are_read_and_the_iso_can_be_changed() {
    // From a cargo test run no bundle exists, so this changes nothing: Homebrew's paths are used.
    configure_library_paths();

    let camera = GphotoCamera::new().expect("the worker starts");
    let found = camera
        .connect()
        .expect("the camera is found and claimed (see the notes at the top)");
    println!("camera: {} ({})", found.name, found.id);

    let finder = camera.finder();
    let before = finder.settings().expect("the settings are read");
    print("settings:", &before);
    assert_eq!(before.len(), CameraSettingKind::ALL.len());

    if let Ok(value) = std::env::var("GTP_GPHOTO_SET_ISO") {
        let after = finder
            .set_setting(CameraSettingKind::Iso, &value)
            .expect("the ISO is set");
        print(&format!("after setting the ISO to {value}:"), &after);
        assert_eq!(after[0].value.as_deref(), Some(value.trim()));
    }
}
