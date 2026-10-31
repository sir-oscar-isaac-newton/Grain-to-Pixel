//! `benchmark golden [--bless] [--dir <folder>] [--maps <folder>]`
//!
//! Without `--bless`, compares every golden case with its stored render and exits with 1 when one
//! differs by more than the limit; `--maps` writes a ΔE map of each differing case. With
//! `--bless`, rewrites the stored renders and prints how far each one moved, for the pull request.
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
use gtp_benchmark::{bless, check, Outcome, CASES, MAX_DELTA_E};

const USAGE: &str = "usage: benchmark golden [--bless] [--dir <folder>] [--maps <folder>]\n       \
                     benchmark fixture --dir <folder> [--frames <n>] [--rolls <n>]";

enum Command {
    Golden {
        bless: bool,
        dir: PathBuf,
        maps: Option<PathBuf>,
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

fn run() -> Result<bool, Box<dyn Error>> {
    match parse(std::env::args().skip(1))? {
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
