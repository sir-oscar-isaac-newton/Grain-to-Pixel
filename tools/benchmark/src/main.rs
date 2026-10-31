//! `benchmark golden [--bless] [--dir <folder>] [--maps <folder>]`
//!
//! Without `--bless`, compares every golden case with its stored render and exits with 1 when one
//! differs by more than the limit; `--maps` writes a ΔE map of each differing case. With
//! `--bless`, rewrites the stored renders and prints how far each one moved, for the pull request.

use std::error::Error;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use gtp_benchmark::golden::default_dir;
use gtp_benchmark::{bless, check, Outcome, CASES, MAX_DELTA_E};

const USAGE: &str = "usage: benchmark golden [--bless] [--dir <folder>] [--maps <folder>]";

struct Args {
    bless: bool,
    dir: PathBuf,
    maps: Option<PathBuf>,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    if args.next().as_deref() != Some("golden") {
        return Err(USAGE.into());
    }
    let mut parsed = Args {
        bless: false,
        dir: default_dir(),
        maps: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bless" => parsed.bless = true,
            "--dir" => parsed.dir = args.next().ok_or(USAGE)?.into(),
            "--maps" => parsed.maps = Some(args.next().ok_or(USAGE)?.into()),
            other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
        }
    }
    Ok(parsed)
}

fn describe(o: &Outcome) -> String {
    match o.diff {
        None => "new".into(),
        Some(d) => format!("mean {:.3}  p95 {:.3}  max {:.3}", d.mean, d.p95, d.max),
    }
}

fn run() -> Result<bool, Box<dyn Error>> {
    let args = parse(std::env::args().skip(1))?;
    let mut out = std::io::stdout().lock();
    let outcomes = if args.bless {
        bless(&args.dir)?
    } else {
        check(&args.dir, args.maps.as_deref())?
    };
    writeln!(
        out,
        "{} golden cases in {} (ΔE2000, limit max {MAX_DELTA_E})",
        if args.bless { "Blessed" } else { "Checked" },
        args.dir.display()
    )?;
    for (o, case) in outcomes.iter().zip(CASES) {
        let verdict = match (args.bless, o.passed()) {
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
    Ok(args.bless || outcomes.iter().all(Outcome::passed))
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

    fn args(list: &[&str]) -> Result<Args, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_the_flags() {
        let a = args(&["golden", "--bless", "--dir", "x", "--maps", "m"]).unwrap();
        assert!(a.bless);
        assert_eq!(a.dir, PathBuf::from("x"));
        assert_eq!(a.maps, Some(PathBuf::from("m")));
        let plain = args(&["golden"]).unwrap();
        assert!(!plain.bless && plain.maps.is_none());
    }

    #[test]
    fn rejects_a_missing_command_or_value_and_unknown_flags() {
        assert!(args(&[]).is_err());
        assert!(args(&["golden", "--dir"]).is_err());
        assert!(args(&["golden", "--fast"]).is_err());
    }
}
