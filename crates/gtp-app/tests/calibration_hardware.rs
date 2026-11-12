//! Needs a real Scanlight V4 on USB and a Nikon D600 on USB, with clear film base in the gate, and
//! macOS with libgphoto2 (`brew install libgphoto2 pkgconf`), so it is ignored and built only with
//! the `tether` feature on macOS. Run it on the Mac with
//!
//! ```text
//! cargo test -p gtp-app --features tether --test calibration_hardware -- --ignored --nocapture
//! ```
//!
//! It runs the automatic calibration (H-17) through the same service as the app's job
//! (`start_calibration`), over the whole frame, and prints the levels, the reached values and the
//! shots per colour. Optional: `GTP_SCANLIGHT_PORT=/dev/cu.usbmodem1101` (else the serial ports are
//! probed) and `GTP_CALIBRATE_LEVELS=44,22,2` (the levels to start from; these are the defaults).
//! The camera must be switched on and free of macOS's own camera helper (log other accounts out,
//! quit Photos and Image Capture). Nothing is saved.
#![cfg(all(feature = "tether", target_os = "macos"))]
// Test code (convention: tests may unwrap); clippy.toml only exempts `#[test]` functions, not the
// helpers around them.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use gtp_app::services::capture_control::{
    light_connect, light_off, start_calibration, CaptureEvents, StepState,
};
use gtp_app::services::roll_frame::{roll_create, RollInput};
use gtp_app::shared_light::LightReading;
use gtp_app::{AppContext, Hardware, JobManager, JobSink};
use gtp_core::testing::TempDir;
use gtp_core::{AppError, JobId, JobResult, TripletChannel};

/// How long the whole calibration may take: 18 shots at most, each with a settle and a download.
const WAIT: Duration = Duration::from_secs(300);

enum Outcome {
    Done(JobResult),
    Error(AppError),
}

struct Collect(Mutex<mpsc::Sender<Outcome>>);

impl JobSink for Collect {
    fn progress(&self, _: JobId, stage: &str, _: f32) {
        print(stage);
    }
    fn done(&self, _: JobId, result: JobResult) {
        let _ = self.0.lock().unwrap().send(Outcome::Done(result));
    }
    fn error(&self, _: JobId, error: AppError) {
        let _ = self.0.lock().unwrap().send(Outcome::Error(error));
    }
}

struct Quiet;

impl CaptureEvents for Quiet {
    fn step(&self, _: TripletChannel, _: StepState) {}
    fn light_status(&self, _: &LightReading) {}
}

// A hardware test prints what it found for the person running it, as the `gtp-capture` hardware
// tests do.
#[allow(clippy::print_stdout)]
fn print(line: &str) {
    println!("{line}");
}

fn start_levels() -> [u32; 3] {
    match std::env::var("GTP_CALIBRATE_LEVELS") {
        Ok(text) => {
            let parsed: Vec<u32> = text
                .split(',')
                .map(|n| n.trim().parse().expect("GTP_CALIBRATE_LEVELS is r,g,b"))
                .collect();
            parsed
                .try_into()
                .expect("GTP_CALIBRATE_LEVELS has three numbers")
        }
        Err(_) => [44, 22, 2],
    }
}

#[test]
#[ignore = "needs a real Scanlight V4 and a Nikon D600"]
fn a_real_calibration_finds_levels_and_leaves_the_light_off() {
    // From a cargo test run no bundle exists, so this changes nothing: Homebrew's paths are used.
    gtp_app::prepare_camera_libraries();

    let dir = TempDir::new("calibration-hw");
    let (ctx, _) = AppContext::new(&dir.join("settings.json"));
    ctx.create_library(&dir.join("lib"), "Calibration").unwrap();
    let roll = roll_create(
        &ctx.library().unwrap(),
        RollInput {
            name: "Calibration".into(),
            ..Default::default()
        },
    )
    .unwrap()
    .id;

    // The real light and the real camera (libgphoto2), as the app builds them.
    let hardware = Hardware::from_vars(None, None);
    assert!(hardware.camera.is_some(), "no camera backend in this build");
    let port = std::env::var("GTP_SCANLIGHT_PORT").ok();
    let info = light_connect(&hardware, port.as_deref()).expect("the light answers");
    print(&format!("light: {info:?}"));

    let (tx, outcomes) = mpsc::channel();
    let jobs = JobManager::new(Arc::new(Collect(Mutex::new(tx))));
    let started = start_levels();
    print(&format!("starting from {started:?}"));
    start_calibration(&ctx, &jobs, &hardware, Arc::new(Quiet), roll, started, None)
        .expect("the calibration starts");
    let outcome = outcomes.recv_timeout(WAIT).expect("the calibration ends");

    // Whatever happened, the light must be off, and a switch-off must be accepted.
    light_off(&hardware).expect("the light switches off");
    assert!(!hardware.busy().is_busy(), "the job released the busy flag");
    assert!(!hardware.light.is_tripped());

    match outcome {
        Outcome::Done(JobResult::Calibrated(result)) => {
            for (i, name) in ["red", "green", "blue"].into_iter().enumerate() {
                print(&format!(
                    "{name}: level {}, reached {:.3}, {} shots",
                    result.levels[i], result.reached[i], result.shots[i]
                ));
            }
            print(&format!("warnings: {:?}", result.warnings));
            assert!(result.levels.iter().all(|l| *l >= 1));
            assert!(result.shots.iter().all(|n| (1..=6).contains(n)));
        }
        Outcome::Done(_) => panic!("the job did not end with a calibration"),
        Outcome::Error(err) => panic!("the calibration failed: {}", err.message),
    }
    let _ = std::fs::remove_dir_all(&dir);
}
