//! The hardware check of `NkscanSource` (H-02): scans a few frames of the roll in the LS-50 and
//! says what came back. Not run by the tests; it needs the scanner.
//!
//! ```text
//! cargo run --release -p gtp-ingest --example nkscan_roll -- [first] [last] [dpi]
//! ```
//!
//! The defaults are frames 1 to 3 at 2000 dpi, with infrared. `GTP_NKSCAN` names the nkscan program
//! when it is not on `PATH`.

use std::io::Write;
use std::time::Instant;

use gtp_ingest::{CancelToken, NkscanSource, ScanParams, ScannerSource};

fn say(text: &str) {
    let _ = writeln!(std::io::stdout(), "{text}");
}

fn main() {
    let arg = |i: usize, default: u32| {
        std::env::args()
            .nth(i)
            .and_then(|a| a.parse().ok())
            .unwrap_or(default)
    };
    let (first, last, dpi) = (arg(1, 1), arg(2, 3), arg(3, 2000));
    let source = NkscanSource::from_env();

    let devices = match source.list_devices() {
        Ok(devices) => devices,
        Err(err) => {
            say(&format!("no scanner: {err:?}"));
            std::process::exit(1);
        }
    };
    for device in &devices {
        say(&format!("scanner {} ({})", device.name, device.id));
    }
    let mut params = ScanParams::new(devices[0].id.clone(), first..=last);
    params.dpi = dpi;

    let start = Instant::now();
    let result = source.scan(
        &params,
        &mut |number, scan| {
            let (lo, hi) = scan
                .rgb16
                .iter()
                .fold((u16::MAX, 0), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
            let ir = scan.ir16.as_ref().map_or("no infrared".to_owned(), |ir| {
                let (lo, hi) = ir
                    .iter()
                    .fold((u16::MAX, 0), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
                format!("infrared {lo} to {hi}")
            });
            say(&format!(
                "frame {number}: {} x {} at {} dpi, colour {lo} to {hi}, {ir}, after {:.0} s",
                scan.width,
                scan.height,
                scan.dpi,
                start.elapsed().as_secs_f32()
            ));
            Ok(())
        },
        &|fraction| say(&format!("progress {:.0} %", fraction * 100.0)),
        &CancelToken::default(),
    );
    match result {
        Ok(()) => say("done; run `nkscan eject` to take the film out"),
        Err(err) => {
            say(&format!("scan failed: {err:?}"));
            std::process::exit(1);
        }
    }
}
