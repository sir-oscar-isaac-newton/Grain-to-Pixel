//! R-06: can nkscan scan a frame at a position of our own choosing, and how do the thumbnail's columns
//! map to scanner addresses? Needs the LS-50 with the film loaded.
//!
//! ```text
//! cargo run --release -- table                      what nkscan finds, and how thumbnail columns map to addresses
//! cargo run --release -- shift <frame> <addresses>  scans <frame> as found and again moved by <addresses>
//!                                                   (negative moves it towards the start of the roll)
//! ```
//!
//! Everything it learns is printed; the two scans of `shift` are also written as 16-bit PGM files of the
//! green plane (rows: along the sensor, columns: along the film) to the folder `probe-out`.

use std::error::Error;
use std::io::Write;
use std::path::Path;

use nkscan::{
    device,
    protocol::{data::Rect, decode::Samples},
    scan::{
        focus::Focus,
        frame::{self, Options},
        framing::{self, Framing},
        meter::Metering,
        pass::Pass,
        profile::Film,
        window::Recipe,
    },
    session::Session,
};

/// The resolution of the two scans of `shift`: coarse, because only where the picture sits matters.
const SCAN_DPI: u16 = 1000;
/// How far (in scanned lines) the two scans are compared for a shift.
const MAX_LAG: i64 = 400;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    pollster::block_on(run(&args))
}

async fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let command = args.first().map(String::as_str).unwrap_or("");
    let shift: Option<(usize, i64)> = match command {
        "table" => None,
        "shift" => {
            let frame = args.get(1).and_then(|a| a.parse().ok());
            let by = args.get(2).and_then(|a| a.parse().ok());
            match (frame, by) {
                (Some(f), Some(b)) if f >= 1 => Some((f, b)),
                _ => return Err("usage: shift <frame, from 1> <addresses, may be negative>".into()),
            }
        }
        _ => return Err("usage: table | shift <frame> <addresses>".into()),
    };

    let (_, transport) = device::Selector::Only.open().await?;
    let mut session = Session::open(transport).await?;
    println!("connected");

    match session.refresh().await {
        Ok(()) | Err(nkscan::error::Error::Media(_)) => {}
        Err(e) => return Err(e.into()),
    }
    if !session.media_loaded().await? {
        return Err("no film is loaded: load the strip and run again".into());
    }
    session.stage().await?;

    let film = Film::Negative;
    let mut samples = Samples::default();
    let discovery = framing::discover(&mut session, None, film, &mut samples).await?;

    let caps = session.capabilities();
    let y = &caps.address.y_axis;
    let optical_dpi = y.optical_dpi;
    println!("framing mechanism: {:?}", Framing::choose(caps));
    println!(
        "y axis: optical {} dpi, boundary {}, addresses {}..={}",
        y.optical_dpi, y.boundary, y.address_range.start, y.address_range.last
    );
    println!("frames found: {}", discovery.frames.len());
    for (i, f) in discovery.frames.iter().enumerate() {
        println!(
            "  frame {:>2}: top {:>7} bottom {:>7} (length {}) left {} right {}",
            i + 1,
            f.top,
            f.bottom,
            f.bottom - f.top,
            f.left,
            f.right
        );
    }
    let tops: Vec<i64> = discovery.frames.iter().map(|f| i64::from(f.top)).collect();
    if tops.len() > 1 {
        let gaps: Vec<i64> = tops.windows(2).map(|w| w[1] - w[0]).collect();
        println!(
            "  distance between frame tops: min {} max {}",
            gaps.iter().min().unwrap_or(&0),
            gaps.iter().max().unwrap_or(&0)
        );
    }
    match &discovery.thumbnail {
        Some(t) => println!(
            "thumbnail: {} sensor rows x {} columns along the film",
            t.rows, t.cols
        ),
        None => println!("thumbnail: none"),
    }
    println!("line pitch: {:?}", discovery.line_pitch);
    match &discovery.column_addresses {
        Some(a) if !a.is_empty() => {
            let steps: Vec<i64> = a
                .windows(2)
                .map(|w| i64::from(w[1]) - i64::from(w[0]))
                .collect();
            println!(
                "column addresses: {} columns, first {}, last {}, step min {} max {}",
                a.len(),
                a[0],
                a[a.len() - 1],
                steps.iter().min().unwrap_or(&0),
                steps.iter().max().unwrap_or(&0)
            );
            // Where does each frame top fall on the thumbnail?
            for (i, top) in tops.iter().enumerate() {
                let column = a.iter().position(|&addr| i64::from(addr) >= *top);
                println!(
                    "  frame {:>2} starts at thumbnail column {:?}",
                    i + 1,
                    column
                );
            }
        }
        _ => println!("column addresses: none"),
    }

    let Some((n, by)) = shift else {
        return Ok(());
    };
    let base: Rect = *discovery.frames.get(n - 1).ok_or_else(|| {
        format!(
            "frame {n} was not found (there are {})",
            discovery.frames.len()
        )
    })?;
    let moved = |by: i64| -> Result<u32, Box<dyn Error>> {
        u32::try_from(i64::from(base.top) + by)
            .map_err(|_| "that moves the frame before address 0".into())
    };
    let moved_top = moved(by)?;
    let moved_bottom = moved(by + i64::from(base.bottom - base.top))?;
    let shifted = Rect {
        top: moved_top,
        bottom: moved_bottom,
        ..base
    };

    let caps = session.capabilities();
    let recipe = Recipe::new(caps, Some(SCAN_DPI), 1, false, false);
    let metering = Metering {
        lock_white_balance: Metering::locks_white_balance(film),
        base_density: discovery.base_density,
        ..Metering::new(film)
    };

    println!("scanning frame {n} as found: {base:?}");
    let first = frame::scan_frame(
        &mut session,
        &recipe,
        base,
        Options {
            exposures: None,
            metering,
            clean: false,
            focus: Focus::default(),
        },
        &mut samples,
    )
    .await?;
    let plane_a = green(&samples, &first.pass);
    describe("as found", &first.pass);

    println!("scanning frame {n} moved by {by} addresses: {shifted:?}");
    let second = frame::scan_frame(
        &mut session,
        &recipe,
        shifted,
        Options {
            exposures: Some(&first.exposures),
            metering,
            clean: false,
            focus: Focus::default(),
        },
        &mut samples,
    )
    .await;
    let second = match second {
        Ok(second) => second,
        Err(e) => {
            println!("the shifted scan failed: {e}");
            return Err(e.into());
        }
    };
    let plane_b = green(&samples, &second.pass);
    describe("moved", &second.pass);

    std::fs::create_dir_all("probe-out")?;
    write_pgm(Path::new("probe-out/as-found.pgm"), &plane_a, &first.pass)?;
    write_pgm(Path::new("probe-out/moved.pgm"), &plane_b, &second.pass)?;
    println!("wrote probe-out/as-found.pgm and probe-out/moved.pgm");

    if (first.pass.rows, first.pass.cols) != (second.pass.rows, second.pass.cols) {
        println!("the two scans differ in size, so they are not compared");
        return Ok(());
    }
    let step = f64::from(optical_dpi) / f64::from(SCAN_DPI);
    let (lag, score) = best_lag(
        &profile(&plane_a, &first.pass),
        &profile(&plane_b, &second.pass),
    );
    println!(
        "the picture moved by {lag} lines along the film (correlation {score:.3}); that is {:.0} addresses, \
         asked for {by} (one line is {step:.2} addresses)",
        lag as f64 * step
    );
    Ok(())
}

fn describe(what: &str, pass: &Pass) {
    println!(
        "  {what}: {} sensor rows x {} lines, complete {}, {} blocks",
        pass.rows, pass.cols, pass.complete, pass.blocks
    );
}

/// The green plane of the last pass, rows of sensor pixels by columns along the film.
fn green(samples: &Samples, pass: &Pass) -> Vec<u16> {
    let plane = samples.colors.get(1).or_else(|| samples.colors.first());
    plane
        .map(|p| p[..(pass.rows * pass.cols).min(p.len())].to_vec())
        .unwrap_or_default()
}

/// The mean brightness of each line along the film.
fn profile(plane: &[u16], pass: &Pass) -> Vec<f64> {
    (0..pass.cols)
        .map(|x| {
            let sum: f64 = (0..pass.rows)
                .map(|y| f64::from(plane[y * pass.cols + x]))
                .sum();
            sum / pass.rows.max(1) as f64
        })
        .collect()
}

/// The lag (in lines) at which `b` best matches `a`, positive when `b` shows what `a` shows
/// that many lines further along, and the Pearson correlation there.
fn best_lag(a: &[f64], b: &[f64]) -> (i64, f64) {
    let mut best = (0, f64::MIN);
    for lag in -MAX_LAG..=MAX_LAG {
        let pairs: Vec<(f64, f64)> = (0..a.len() as i64)
            .filter_map(|i| {
                let j = i - lag;
                (j >= 0 && (j as usize) < b.len()).then(|| (a[i as usize], b[j as usize]))
            })
            .collect();
        if pairs.len() < a.len() / 2 {
            continue;
        }
        let n = pairs.len() as f64;
        let (ma, mb) = pairs
            .iter()
            .fold((0.0, 0.0), |s, p| (s.0 + p.0 / n, s.1 + p.1 / n));
        let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
        for (x, y) in &pairs {
            cov += (x - ma) * (y - mb);
            va += (x - ma).powi(2);
            vb += (y - mb).powi(2);
        }
        let score = if va > 0.0 && vb > 0.0 {
            cov / (va * vb).sqrt()
        } else {
            0.0
        };
        if score > best.1 {
            best = (lag, score);
        }
    }
    best
}

fn write_pgm(path: &Path, plane: &[u16], pass: &Pass) -> Result<(), Box<dyn Error>> {
    let mut file = std::fs::File::create(path)?;
    write!(file, "P5\n{} {}\n65535\n", pass.cols, pass.rows)?;
    let mut bytes = Vec::with_capacity(plane.len() * 2);
    for v in plane {
        bytes.extend_from_slice(&v.to_be_bytes());
    }
    file.write_all(&bytes)?;
    Ok(())
}
