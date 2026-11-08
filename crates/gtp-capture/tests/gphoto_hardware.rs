//! Needs a real Scanlight V4 and a Nikon D600 on USB, and macOS with libgphoto2 (`brew install
//! libgphoto2 pkgconf`), so it is ignored and built only with the `tether` feature on macOS. Run it on
//! the Mac with
//!
//! ```text
//! cargo test -p gtp-capture --features tether --test gphoto_hardware -- --ignored --nocapture
//! ```
//!
//! Optional: `GTP_SCANLIGHT_PORT=/dev/cu.usbmodem1101`, and `GTP_GPHOTO_OUT=<folder>` for where the
//! three NEFs are written (default: a folder in the system's temp directory, printed at the end).
//! The camera must be in M mode, manual focus, RAW, switched on and free of macOS's own camera
//! helper (log other accounts out, quit Photos and Image Capture), with the lens cap off. The
//! Scanlight lights red, green and blue in turn at the default levels (44, 22, 2); about ten
//! seconds in all.
#![cfg(all(feature = "tether", target_os = "macos"))]

use std::path::PathBuf;

use gtp_capture::{
    configure_library_paths, CameraSource, CaptureProfile, CaptureSequence, GphotoCamera,
    LightSource, ScanlightV4,
};

#[test]
#[ignore = "needs a real Scanlight V4 and a Nikon D600"]
fn a_real_triplet_is_captured_and_downloaded_through_the_sequence_and_the_light_ends_off() {
    // From a cargo test run no bundle exists, so this changes nothing: Homebrew's paths are used.
    configure_library_paths();

    let mut light = ScanlightV4::new();
    let port = std::env::var("GTP_SCANLIGHT_PORT").ok();
    let info = light.connect(port.as_deref()).expect("the light connects");
    assert!(info.recognised, "unexpected hardware ID: {info:?}");

    let mut camera = GphotoCamera::new().expect("the worker starts");
    let found = camera
        .connect()
        .expect("the camera is found and claimed (see the notes at the top)");
    println!("camera: {} ({})", found.name, found.id);
    assert!(camera.is_connected());

    let sequence = CaptureSequence::new(CaptureProfile::default());
    let outcome = sequence
        .run(&mut light, &mut camera, &|| false, &mut |channel| {
            println!("picture in: {channel:?}");
        })
        .expect("the triplet is captured");

    let out = std::env::var_os("GTP_GPHOTO_OUT").map_or_else(
        || std::env::temp_dir().join("gtp-gphoto-hardware"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&out).expect("the output folder");
    for (name, file) in ["red", "green", "blue"].iter().zip(&outcome.files) {
        assert!(
            file.bytes.len() > 1_000_000,
            "{name}: {} bytes is not a NEF",
            file.bytes.len()
        );
        let path = out.join(format!("{name}-{}", file.name));
        std::fs::write(&path, &file.bytes).expect("writing a NEF");
        println!("{name}: {} bytes, {}", file.bytes.len(), path.display());
    }
    println!("warnings: {:?}", outcome.warnings);

    // The sequence's guard sent all zeros. The light is open loop (it cannot be asked its levels),
    // so also say it again and check it is still answering; the human looks at the LEDs.
    assert!(light.status().connected, "the light is still connected");
    light.set_levels([0, 0, 0]).expect("the light accepts off");
    println!("CHECK BY EYE: all three LEDs of the Scanlight must be dark now.");
}
