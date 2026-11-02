//! `benchmark golden [--bless] [--dir <folder>] [--maps <folder>]`
//!
//! Without `--bless`, compares every golden case with its stored render and exits with 1 when one
//! differs by more than the limit; `--maps` writes a ΔE map of each differing case. With
//! `--bless`, rewrites the stored renders and prints how far each one moved, for the pull request.
//!
//! `benchmark report --manifest <file> [--maps <folder>]`
//!
//! Converts each camera triplet of the manifest the way the app does, registers the reference render
//! of the same frame in it, and prints the CIEDE2000 difference per frame and for all of them
//! (median and 95th percentile). `--maps` writes ours, the reference and the difference side by
//! side for each frame. The pairs are the author's private samples (`samples/private/reference`).
//!
//! `benchmark manifest --triplets <folder> --references <folder> --out <file> [--format <f>]`
//!
//! Writes a manifest for a folder of camera files and a folder of reference renders: triplets in
//! capture order matched to the references in file name order. Check the note it prints.
//!
//! `benchmark fixture --dir <folder> [--frames <n>] [--rolls <n>]`
//!
//! Builds a library of synthetic frames (5,000 in 100 rolls by default) to open in the app and
//! scroll, search and tag at the size the Library view is designed for. The folder must not hold a
//! library already.

use std::error::Error;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use gtp_benchmark::fixture::{self, DEFAULT_FRAMES, DEFAULT_ROLLS};
use gtp_benchmark::golden::default_dir;
use gtp_benchmark::manifest;
use gtp_benchmark::report::{self, Report, MIN_SCORE};
use gtp_benchmark::{bless, check, Outcome, CASES, MAX_DELTA_E};

const USAGE: &str = "usage: benchmark golden [--bless] [--dir <folder>] [--maps <folder>]\n       \
                     benchmark report --manifest <file> [--maps <folder>]\n       \
benchmark manifest --triplets <folder> --references <folder> --out <file> [--format <f>]\n       \
                     benchmark fixture --dir <folder> [--frames <n>] [--rolls <n>]";

enum Command {
    Golden {
        bless: bool,
        dir: PathBuf,
        maps: Option<PathBuf>,
    },
    Report {
        manifest: PathBuf,
        maps: Option<PathBuf>,
    },
    Manifest {
        triplets: PathBuf,
        references: PathBuf,
        out: PathBuf,
        format: String,
    },
    Fixture {
        dir: PathBuf,
        frames: u32,
        rolls: u32,
    },
}

fn number(value: Option<String>) -> Result<u32, String> {
    value
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| USAGE.to_string())
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    match args.next().as_deref() {
        Some("golden") => {
            let (mut bless, mut dir, mut maps) = (false, default_dir(), None);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--bless" => bless = true,
                    "--dir" => dir = args.next().ok_or(USAGE)?.into(),
                    "--maps" => maps = Some(args.next().ok_or(USAGE)?.into()),
                    other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
                }
            }
            Ok(Command::Golden { bless, dir, maps })
        }
        Some("report") => {
            let (mut manifest, mut maps) = (None, None);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--manifest" => manifest = Some(args.next().ok_or(USAGE)?.into()),
                    "--maps" => maps = Some(args.next().ok_or(USAGE)?.into()),
                    other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
                }
            }
            Ok(Command::Report {
                manifest: manifest.ok_or_else(|| format!("--manifest is required\n{USAGE}"))?,
                maps,
            })
        }
        Some("manifest") => {
            let (mut triplets, mut references, mut out) = (None, None, None);
            let mut format = "645".to_string();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--triplets" => triplets = Some(args.next().ok_or(USAGE)?.into()),
                    "--references" => references = Some(args.next().ok_or(USAGE)?.into()),
                    "--out" => out = Some(args.next().ok_or(USAGE)?.into()),
                    "--format" => format = args.next().ok_or(USAGE)?,
                    other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
                }
            }
            let need = |v: Option<PathBuf>, name: &str| {
                v.ok_or_else(|| format!("--{name} is required\n{USAGE}"))
            };
            Ok(Command::Manifest {
                triplets: need(triplets, "triplets")?,
                references: need(references, "references")?,
                out: need(out, "out")?,
                format,
            })
        }
        Some("fixture") => {
            let (mut dir, mut frames, mut rolls) = (None, DEFAULT_FRAMES, DEFAULT_ROLLS);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--dir" => dir = Some(args.next().ok_or(USAGE)?.into()),
                    "--frames" => frames = number(args.next())?,
                    "--rolls" => rolls = number(args.next())?,
                    other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
                }
            }
            Ok(Command::Fixture {
                dir: dir.ok_or_else(|| format!("--dir is required\n{USAGE}"))?,
                frames,
                rolls,
            })
        }
        _ => Err(USAGE.into()),
    }
}

fn describe(o: &Outcome) -> String {
    match o.diff {
        None => "new".into(),
        Some(d) => format!("mean {:.3}  p95 {:.3}  max {:.3}", d.mean, d.p95, d.max),
    }
}

fn golden(bless_cases: bool, dir: PathBuf, maps: Option<PathBuf>) -> Result<bool, Box<dyn Error>> {
    let mut out = std::io::stdout().lock();
    let outcomes = if bless_cases {
        bless(&dir)?
    } else {
        check(&dir, maps.as_deref())?
    };
    writeln!(
        out,
        "{} golden cases in {} (ΔE2000, limit max {MAX_DELTA_E})",
        if bless_cases { "Blessed" } else { "Checked" },
        dir.display()
    )?;
    for (o, case) in outcomes.iter().zip(CASES) {
        let verdict = match (bless_cases, o.passed()) {
            (true, _) if o.diff.is_none() => "added",
            (true, _) if o.diff.is_some_and(|d| d.max == 0.0) => "unchanged",
            (true, _) => "updated",
            (false, true) => "ok",
            (false, false) => "FAIL",
        };
        writeln!(
            out,
            "  {verdict:<9} {:<16} {:<40} {}",
            o.name,
            describe(o),
            case.about
        )?;
    }
    Ok(bless_cases || outcomes.iter().all(Outcome::passed))
}

/// The report as the command prints it.
fn print_report(report: &Report, out: &mut impl Write) -> std::io::Result<()> {
    let [r, g, b] = report.film_base;
    writeln!(
        out,
        "Film base {r:.3} {g:.3} {b:.3}; colour difference is CIEDE2000, sRGB."
    )?;
    writeln!(
        out,
        "{:<6} {:>6} {:>8} {:>6} {:>6}   {:>14}",
        "frame", "match", "crop gap", "median", "p95", "cast L* a* b*"
    )?;
    for f in &report.frames {
        let number = |v: Option<f32>, width: usize, precision: usize| {
            v.map_or(format!("{:>width$}", "-"), |v| {
                format!("{v:>width$.precision$}")
            })
        };
        let cast = f.cast.map_or("-".to_string(), |c| {
            format!("{:+.1} {:+.1} {:+.1}", c[0], c[1], c[2])
        });
        let note = match f.score {
            None => "  (flat picture, not registered)",
            Some(s) if s < MIN_SCORE => "  (poor match: not the same frame?)",
            Some(_) => "",
        };
        writeln!(
            out,
            "{:<6} {} {} {} {}   {:>14}{note}",
            f.name,
            number(f.score, 6, 3),
            number(f.crop_gap.map(|g| g * 100.0), 7, 1) + "%",
            number(f.median, 6, 1),
            number(f.p95, 6, 1),
            cast
        )?;
    }
    let counted = report.frames.iter().filter(|f| f.median.is_some()).count();
    writeln!(
        out,
        "{counted} of {} frames compared. All pixels: median {:.1}, p95 {:.1}. Median of the frames' medians {:.1}.",
        report.frames.len(),
        report.pooled_median,
        report.pooled_p95,
        report.median_of_medians
    )
}

fn run() -> Result<bool, Box<dyn Error>> {
    match parse(std::env::args().skip(1))? {
        Command::Manifest {
            triplets,
            references,
            out,
            format,
        } => {
            let (text, note) = manifest::build(&triplets, &references, &format)?;
            std::fs::write(&out, text)?;
            writeln!(std::io::stdout().lock(), "{note}\nWrote {}", out.display())?;
            Ok(true)
        }
        Command::Report { manifest, maps } => {
            let manifest = manifest::read(&manifest)?;
            let mut note = |line: &str| {
                let _ = writeln!(std::io::stderr(), "{line}");
            };
            let report = report::run(&manifest, maps.as_deref(), &mut note)?;
            print_report(&report, &mut std::io::stdout().lock())?;
            Ok(true)
        }
        Command::Golden { bless, dir, maps } => golden(bless, dir, maps),
        Command::Fixture { dir, frames, rolls } => {
            let started = Instant::now();
            let built = fixture::build(&dir, frames, rolls)?;
            writeln!(
                std::io::stdout().lock(),
                "Built a library of {} frames in {} rolls in {} ({:.1} s). Open that folder in Grain to Pixel.",
                built.frames,
                built.rolls,
                dir.display(),
                started.elapsed().as_secs_f32()
            )?;
            Ok(true)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "{err}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Command, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_the_golden_flags() {
        let Ok(Command::Golden { bless, dir, maps }) =
            args(&["golden", "--bless", "--dir", "x", "--maps", "m"])
        else {
            unreachable!("golden should parse");
        };
        assert!(bless);
        assert_eq!(dir, PathBuf::from("x"));
        assert_eq!(maps, Some(PathBuf::from("m")));
        let Ok(Command::Golden { bless, maps, .. }) = args(&["golden"]) else {
            unreachable!("golden should parse");
        };
        assert!(!bless && maps.is_none());
    }

    #[test]
    fn parses_the_fixture_command_with_defaults() {
        let Ok(Command::Fixture { dir, frames, rolls }) = args(&["fixture", "--dir", "lib"]) else {
            unreachable!("fixture should parse");
        };
        assert_eq!(dir, PathBuf::from("lib"));
        assert_eq!((frames, rolls), (DEFAULT_FRAMES, DEFAULT_ROLLS));
        let Ok(Command::Fixture { frames, rolls, .. }) =
            args(&["fixture", "--dir", "lib", "--frames", "60", "--rolls", "3"])
        else {
            unreachable!("fixture should parse");
        };
        assert_eq!((frames, rolls), (60, 3));
    }

    #[test]
    fn parses_the_report_and_manifest_commands() {
        let Ok(Command::Report { manifest, maps }) =
            args(&["report", "--manifest", "m.json", "--maps", "out"])
        else {
            unreachable!("report should parse");
        };
        assert_eq!(
            (manifest, maps),
            (PathBuf::from("m.json"), Some(PathBuf::from("out")))
        );
        assert!(args(&["report"]).is_err(), "a manifest is required");
        let Ok(Command::Manifest { format, out, .. }) = args(&[
            "manifest",
            "--triplets",
            "t",
            "--references",
            "r",
            "--out",
            "m.json",
        ]) else {
            unreachable!("manifest should parse");
        };
        assert_eq!((format.as_str(), out), ("645", PathBuf::from("m.json")));
        assert!(args(&["manifest", "--triplets", "t", "--out", "x"]).is_err());
    }

    #[test]
    fn rejects_a_missing_command_or_value_and_unknown_flags() {
        assert!(args(&[]).is_err());
        assert!(args(&["golden", "--dir"]).is_err());
        assert!(args(&["golden", "--fast"]).is_err());
        assert!(args(&["fixture"]).is_err(), "a folder is required");
        assert!(args(&["fixture", "--dir", "x", "--frames", "many"]).is_err());
        assert!(args(&["fixture", "--dir", "x", "--frames"]).is_err());
        assert!(args(&["fixture", "--dir", "x", "--fast"]).is_err());
    }
}
