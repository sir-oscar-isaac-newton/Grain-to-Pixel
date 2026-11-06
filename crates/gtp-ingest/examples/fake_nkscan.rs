//! A stand-in for the `nkscan` command line, for the tests of `NkscanSource`. It writes small
//! 16-bit TIFFs the way nkscan names them and behaves as the environment says:
//! `FAKE_NKSCAN_VERSION` (default 0.17.0), `FAKE_NKSCAN_FAIL_AT` (fail when this frame is next, with
//! the message in `FAKE_NKSCAN_ERROR`), `FAKE_NKSCAN_DELAY_MS` (wait before each frame),
//! `FAKE_NKSCAN_LIST` (what `list` prints), `FAKE_NKSCAN_NO_IR` (leave the infrared file out),
//! `FAKE_NKSCAN_DETECTED` (the frames it finds, default 6) and `FAKE_NKSCAN_NO_THUMB` (write no
//! thumbnail with `--thumbnail`; `FAKE_NKSCAN_NO_COUNT` leaves out the count).

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use gtp_io::{write_tiff16, Compression, Tiff16};

const W: u32 = 8;
const H: u32 = 6;

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn say(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    match args.first().map(String::as_str) {
        Some("--version") => {
            let version = env("FAKE_NKSCAN_VERSION").unwrap_or_else(|| "0.17.0".into());
            let _ = writeln!(std::io::stdout(), "nkscan {version}");
        }
        Some("list") => {
            let listing = env("FAKE_NKSCAN_LIST")
                .unwrap_or_else(|| "Attached scanners:\n  Nikon LS-50 ED (usb:01-1.4)".into());
            let _ = writeln!(std::io::stdout(), "{listing}");
        }
        Some("eject") => {}
        Some("scan") => scan(&args, &value),
        _ => {
            say("unknown command");
            std::process::exit(2);
        }
    }
}

fn scan(args: &[String], value: &dyn Fn(&str) -> Option<String>) {
    let Some(basename) = value("--basename") else {
        say("no --basename");
        std::process::exit(2);
    };
    let frames: Vec<u32> = value("--frames")
        .unwrap_or_default()
        .split(',')
        .filter_map(|f| f.parse().ok())
        .collect();
    let ir = args.iter().any(|a| a == "--ir") && env("FAKE_NKSCAN_NO_IR").is_none();
    let delay: u64 = env("FAKE_NKSCAN_DELAY_MS")
        .and_then(|d| d.parse().ok())
        .unwrap_or(0);
    let fail_at: Option<usize> = env("FAKE_NKSCAN_FAIL_AT").and_then(|f| f.parse().ok());
    if args.iter().any(|a| a == "--thumbnail") && env("FAKE_NKSCAN_NO_THUMB").is_none() {
        let count: u32 = env("FAKE_NKSCAN_DETECTED")
            .and_then(|c| c.parse().ok())
            .unwrap_or(6);
        if env("FAKE_NKSCAN_NO_COUNT").is_none() {
            say(&format!("INFO detected frames frames={count}"));
        }
        // The strip: 20 columns a frame, 8 rows; the red of a column is 100 times its number.
        let (sw, sh) = (count * 20, 8u32);
        let mut data = Vec::new();
        for _ in 0..sh {
            for x in 0..sw {
                data.extend([(x as u16) * 100, 5, 5]);
            }
        }
        let strip = Tiff16 {
            width: sw,
            height: sh,
            channels: 3,
            data,
        };
        let path = PathBuf::from(format!("{basename}_1_thumbnail.tiff"));
        if write_tiff16(&path, &strip, Compression::None).is_err() {
            say("write failed");
            std::process::exit(1);
        }
    }
    for (i, frame) in frames.iter().enumerate() {
        let n = i + 1;
        std::thread::sleep(Duration::from_millis(delay));
        if fail_at == Some(n) {
            say(&env("FAKE_NKSCAN_ERROR").unwrap_or_else(|| "focus failed".into()));
            std::process::exit(1);
        }
        say(&format!("scanning frame {frame}"));
        // The first sample of a picture is 1000 times the frame number the app asked for, so a test
        // can tell which frame it got.
        let mut data = vec![7u16; (W * H * 3) as usize];
        data[0] = (*frame as u16) * 1000;
        let rgb = Tiff16 {
            width: W,
            height: H,
            channels: 3,
            data,
        };
        let path = PathBuf::from(format!("{basename}_{n}.tiff"));
        if write_tiff16(&path, &rgb, Compression::None).is_err() {
            say("write failed");
            std::process::exit(1);
        }
        if ir {
            let plane = Tiff16 {
                width: W,
                height: H,
                channels: 1,
                data: vec![9u16; (W * H) as usize],
            };
            let path = PathBuf::from(format!("{basename}_{n}_IR.tiff"));
            if write_tiff16(&path, &plane, Compression::None).is_err() {
                say("write failed");
                std::process::exit(1);
            }
        }
    }
}
