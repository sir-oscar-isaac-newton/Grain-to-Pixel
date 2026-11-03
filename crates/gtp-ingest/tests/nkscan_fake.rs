//! `NkscanSource` against a stand-in for the nkscan program (`examples/fake_nkscan.rs`), which writes
//! small TIFFs the way nkscan names them.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use gtp_core::{AppError, ErrorCode};
use gtp_ingest::{CancelToken, NkscanSource, ScanParams, ScannerSource};

fn fake_program() -> PathBuf {
    // target/debug/deps/<test> -> target/debug
    let exe = std::env::current_exe().unwrap_or_default();
    let debug = exe.ancestors().nth(2).unwrap_or(Path::new("."));
    let name = if cfg!(windows) {
        "fake_nkscan.exe"
    } else {
        "fake_nkscan"
    };
    let path = debug.join("examples").join(name);
    assert!(
        path.is_file(),
        "build the example first: {}",
        path.display()
    );
    path
}

fn source() -> NkscanSource {
    NkscanSource::new(fake_program())
}

/// A work folder of its own for a test, so the tests can check nothing is left behind.
fn work_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gtp-nkscan-test-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(std::fs::create_dir_all(&dir).is_ok(), "{}", dir.display());
    dir
}

fn left_in(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |d| d.count())
}

#[test]
fn the_scanner_is_listed_and_the_version_is_checked() {
    let devices = source().list_devices().unwrap();
    assert_eq!(devices[0].id, "usb:01-1.4");

    let wrong = source().with_env("FAKE_NKSCAN_VERSION", "0.18.0");
    let err = wrong.list_devices().unwrap_err();
    assert_eq!(err.code, ErrorCode::ScannerUnavailable);
    assert!(err.message.contains("0.17.0"));

    let held = source().with_env("FAKE_NKSCAN_LIST", "Attached scanners:\n  LS-50 (in use)");
    assert_eq!(
        held.list_devices().unwrap_err().code,
        ErrorCode::ScannerUnavailable
    );
    let none = source().with_env("FAKE_NKSCAN_LIST", "Attached scanners:");
    assert_eq!(
        none.list_devices().unwrap_err().code,
        ErrorCode::ScannerUnavailable
    );

    let missing = NkscanSource::new("no-such-nkscan-program");
    assert_eq!(
        missing.list_devices().unwrap_err().code,
        ErrorCode::ScannerUnavailable
    );
    assert!(source().eject().is_ok());
}

#[test]
fn every_frame_is_handed_over_as_it_finishes_under_the_number_asked_for() {
    let dir = work_dir("frames");
    let mut got: Vec<(u32, u16, bool)> = Vec::new();
    let steps = Mutex::new(Vec::new());
    let mut p = ScanParams::new("usb:01-1.4", 2..=4);
    p.dpi = 2000;
    source()
        .with_work_dir(&dir)
        .scan(
            &p,
            &mut |n, scan| {
                got.push((n, scan.rgb16[0], scan.ir16.is_some()));
                assert_eq!((scan.width, scan.height, scan.dpi), (8, 6, 2000));
                assert_eq!(scan.meta.device_id, "usb:01-1.4");
                Ok(())
            },
            &|f| steps.lock().unwrap().push(f),
            &CancelToken::default(),
        )
        .unwrap();
    // Files 1, 2 and 3 of nkscan are frames 2, 3 and 4 of the roll.
    assert_eq!(got, [(2, 2000, true), (3, 3000, true), (4, 4000, true)]);
    let steps = steps.into_inner().unwrap();
    assert_eq!(steps.first(), Some(&0.0));
    assert_eq!(steps.last(), Some(&1.0));
    assert!(steps.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(left_in(&dir), 0, "the scan folder is removed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_scan_without_infrared_has_none_and_asking_for_it_when_missing_fails() {
    let dir = work_dir("ir");
    let mut p = ScanParams::new("d", 1..=1);
    p.ir = false;
    let mut had_ir = true;
    source()
        .with_work_dir(&dir)
        .scan(
            &p,
            &mut |_, s| {
                had_ir = s.ir16.is_some();
                Ok(())
            },
            &|_| {},
            &CancelToken::default(),
        )
        .unwrap();
    assert!(!had_ir);

    let p = ScanParams::new("d", 1..=1);
    let err = source()
        .with_work_dir(&dir)
        .with_env("FAKE_NKSCAN_NO_IR", "1")
        .scan(&p, &mut |_, _| Ok(()), &|_| {}, &CancelToken::default())
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ScanFailed);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_error_on_frame_k_keeps_the_frames_before_it() {
    let dir = work_dir("fail");
    let delivered = AtomicU32::new(0);
    let err = source()
        .with_work_dir(&dir)
        .with_env("FAKE_NKSCAN_FAIL_AT", "3")
        .with_env("FAKE_NKSCAN_ERROR", "focus failed on frame 3")
        .scan(
            &ScanParams::new("d", 5..=9),
            &mut |_, _| {
                delivered.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            &|_| {},
            &CancelToken::default(),
        )
        .unwrap_err();
    assert_eq!(delivered.load(Ordering::SeqCst), 2);
    assert_eq!(err.code, ErrorCode::ScanFailed);
    assert!(err.details.unwrap().contains("focus failed on frame 3"));
    assert_eq!(left_in(&dir), 0);

    let err = source()
        .with_work_dir(&dir)
        .with_env("FAKE_NKSCAN_FAIL_AT", "1")
        .with_env(
            "FAKE_NKSCAN_ERROR",
            "incompatible driver is installed for this device",
        )
        .scan(
            &ScanParams::new("d", 1..=2),
            &mut |_, _| Ok(()),
            &|_| {},
            &CancelToken::default(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::ScannerUnavailable);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_error_from_the_store_stops_the_scan_with_that_error() {
    let dir = work_dir("store");
    let mut calls = 0;
    let err = source()
        .with_work_dir(&dir)
        .scan(
            &ScanParams::new("d", 1..=3),
            &mut |_, _| {
                calls += 1;
                Err(AppError::new(ErrorCode::Io, "disk full"))
            },
            &|_| {},
            &CancelToken::default(),
        )
        .unwrap_err();
    assert_eq!((err.code, calls), (ErrorCode::Io, 1));
    assert_eq!(left_in(&dir), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancelling_stops_the_program_and_keeps_what_was_handed_over() {
    let dir = work_dir("cancel");
    let cancel = CancelToken::default();
    let mut seen = Vec::new();
    let err = source()
        .with_work_dir(&dir)
        .with_env("FAKE_NKSCAN_DELAY_MS", "400")
        .scan(
            &ScanParams::new("d", 1..=10),
            &mut |n, _| {
                seen.push(n);
                cancel.cancel();
                Ok(())
            },
            &|_| {},
            &cancel,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Cancelled);
    assert_eq!(seen, [1]);
    assert_eq!(left_in(&dir), 0);

    let already = CancelToken::default();
    already.cancel();
    let err = source()
        .scan(
            &ScanParams::new("d", 1..=1),
            &mut |_, _| Ok(()),
            &|_| {},
            &already,
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Cancelled);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_parameters_never_start_the_program() {
    let err = NkscanSource::new("no-such-nkscan-program")
        .scan(
            &ScanParams::new("", 1..=1),
            &mut |_, _| Ok(()),
            &|_| {},
            &CancelToken::default(),
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidInput);
}
