//! `benchmark golden [--bless] [--dir <folder>] [--maps <folder>]`
//!
//! Without `--bless`, compares every golden case with its stored render and exits with 1 when one
//! differs by more than the limit; `--maps` writes a ΔE map of each differing case. With
//! `--bless`, rewrites the stored renders and prints how far each one moved, for the pull request.
//!
//! `benchmark report --manifest <file> [--maps <folder>] [--look neutral|film_log|frontier]
//! [--balance off|grey_world|near_neutral|roll_median]`
//!
//! Converts each frame of the manifest (a camera triplet or an LS-50 scan) the way the app does,
//! registers the reference render of the same frame in it (turned or mirrored if need be), and prints
//! the CIEDE2000 difference per frame, per roll and for all of them (median and 95th percentile). `--maps` writes ours, the reference and the difference side by
//! side for each frame. The pairs are the author's private samples (`samples/private/reference`).
//! `--balance` sets the automatic colour balance (P-22): `off` (the default), one of the two per-frame
//! estimators, or `roll_median`, the median over each roll of the grey-world axes.
//!
//! `benchmark manifest (--triplets <folder> | --scans <folder>) --references <folder> --out <file>
//! [--format <f>]`
//!
//! Writes a manifest for a folder of camera files (triplets in capture order, matched to the
//! references in file name order) or of LS-50 scans (matched to Nikon Scan's positives by name, the
//! stock of each scan being its roll), and a folder of reference renders. Check the note it prints.
//!
//! `benchmark fit --manifest <file> [--holdout <n>] [--percentiles <low,high>]...`
//!
//! Fits the Frontier-style look (a sigmoid tone curve with a gamma, and a saturation) to the pairs of
//! the manifest, holding out every n-th frame (3 by default) and judging the fit on those. Without
//! `--percentiles` it tries the levels at 0.1/99.9, 0.5/99.5 (what the app uses), 1/99 and 2/98.
//! Prints the numbers, ready to paste into `crates/gtp-edit/src/frontier.rs`.
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

use gtp_app::{BalanceMethod, LevelsPercentiles};
use gtp_benchmark::fit::{self, FitReport, Score};
use gtp_benchmark::fixture::{self, DEFAULT_FRAMES, DEFAULT_ROLLS};
use gtp_benchmark::golden::default_dir;
use gtp_benchmark::manifest;
use gtp_benchmark::report::{self, Balance, Report, MIN_SCORE};
use gtp_benchmark::{bless, check, Outcome, CASES, MAX_DELTA_E};
use gtp_core::Look;

const USAGE: &str = "usage: benchmark golden [--bless] [--dir <folder>] [--maps <folder>]\n       \
                     benchmark report --manifest <file> [--maps <folder>] [--look neutral|film_log|frontier] [--balance off|grey_world|near_neutral|roll_median]\n       \
                     benchmark fit --manifest <file> [--holdout <n>] [--percentiles <low,high>]...\n       \
                     benchmark manifest (--triplets <folder> | --scans <folder>) --references <folder> --out <file> [--format <f>]\n       \
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
        look: Look,
        balance: Balance,
    },
    Fit {
        manifest: PathBuf,
        holdout: usize,
        /// The levels percentiles to fit with; empty means the four defaults.
        percentiles: Vec<(f32, f32)>,
    },
    Manifest {
        /// The folder of camera files (`false`) or of LS-50 scans (`true`).
        scans: bool,
        frames: PathBuf,
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

fn parse_balance(text: &str) -> Result<Balance, String> {
    match text {
        "off" => Ok(Balance::Frame(BalanceMethod::Off)),
        "grey_world" => Ok(Balance::Frame(BalanceMethod::GreyWorld)),
        "near_neutral" => Ok(Balance::Frame(BalanceMethod::NearNeutral)),
        "roll_median" => Ok(Balance::RollMedian),
        other => Err(format!(
            "unknown balance {other:?}
{USAGE}"
        )),
    }
}

fn parse_look(text: &str) -> Result<Look, String> {
    match text {
        "neutral" => Ok(Look::Neutral),
        "film_log" => Ok(Look::FilmLog),
        "frontier" => Ok(Look::Frontier),
        other => Err(format!("unknown look {other:?}\n{USAGE}")),
    }
}

/// `0.5,99.5` as a pair of percentiles with the first below the second, both from 0 to 100.
fn parse_percentiles(text: &str) -> Result<(f32, f32), String> {
    let (low, high) = text.split_once(',').ok_or_else(|| USAGE.to_string())?;
    let (low, high): (f32, f32) = (
        low.trim().parse().map_err(|_| USAGE.to_string())?,
        high.trim().parse().map_err(|_| USAGE.to_string())?,
    );
    if !(0.0 <= low && low < high && high <= 100.0) {
        return Err(format!(
            "the percentiles must satisfy 0 <= low < high <= 100\n{USAGE}"
        ));
    }
    Ok((low, high))
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
            let (mut manifest, mut maps, mut look) = (None, None, Look::Neutral);
            let mut balance = Balance::Frame(BalanceMethod::Off);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--manifest" => manifest = Some(args.next().ok_or(USAGE)?.into()),
                    "--maps" => maps = Some(args.next().ok_or(USAGE)?.into()),
                    "--look" => look = parse_look(&args.next().ok_or(USAGE)?)?,
                    "--balance" => balance = parse_balance(&args.next().ok_or(USAGE)?)?,
                    other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
                }
            }
            Ok(Command::Report {
                manifest: manifest.ok_or_else(|| format!("--manifest is required\n{USAGE}"))?,
                maps,
                look,
                balance,
            })
        }
        Some("fit") => {
            let (mut manifest, mut holdout, mut percentiles) = (None, 3usize, Vec::new());
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--manifest" => manifest = Some(args.next().ok_or(USAGE)?.into()),
                    "--holdout" => holdout = number(args.next())? as usize,
                    "--percentiles" => {
                        percentiles.push(parse_percentiles(&args.next().ok_or(USAGE)?)?)
                    }
                    other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
                }
            }
            Ok(Command::Fit {
                manifest: manifest.ok_or_else(|| format!("--manifest is required\n{USAGE}"))?,
                holdout,
                percentiles,
            })
        }
        Some("manifest") => {
            let (mut triplets, mut scans, mut references, mut out) = (None, None, None, None);
            let mut format = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--triplets" => triplets = Some(args.next().ok_or(USAGE)?.into()),
                    "--scans" => scans = Some(args.next().ok_or(USAGE)?.into()),
                    "--references" => references = Some(args.next().ok_or(USAGE)?.into()),
                    "--out" => out = Some(args.next().ok_or(USAGE)?.into()),
                    "--format" => format = Some(args.next().ok_or(USAGE)?),
                    other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
                }
            }
            let need = |v: Option<PathBuf>, name: &str| {
                v.ok_or_else(|| format!("--{name} is required\n{USAGE}"))
            };
            let (frames, is_scans) = match (triplets, scans) {
                (Some(t), None) => (t, false),
                (None, Some(s)) => (s, true),
                _ => return Err(format!("give --triplets or --scans, not both\n{USAGE}")),
            };
            Ok(Command::Manifest {
                scans: is_scans,
                frames,
                references: need(references, "references")?,
                out: need(out, "out")?,
                // Camera triplets are 645 unless said; LS-50 scans are 35 mm.
                format: format.unwrap_or_else(|| if is_scans { "35mm" } else { "645" }.to_string()),
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
    writeln!(out, "Colour difference is CIEDE2000, sRGB.")?;
    for roll in &report.rolls {
        let [r, g, b] = roll.film_base;
        writeln!(out, "{}: film base {r:.3} {g:.3} {b:.3}", roll.roll)?;
    }
    writeln!(
        out,
        "{:<26} {:>6} {:>5} {:>8} {:>6} {:>6}   {:>14} {:>7} {:>11}",
        "frame",
        "match",
        "turn",
        "crop gap",
        "median",
        "p95",
        "cast L* a* b*",
        "chroma",
        "edge cast"
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
        // How the reference was turned: 0, 90, 180 or 270 degrees clockwise, and `m` when mirrored.
        let turn = f.orientation.map_or("-".to_string(), |o| {
            format!(
                "{}{}",
                u32::from(o.quarter_turns % 4) * 90,
                if o.mirrored { "m" } else { "" }
            )
        });
        writeln!(
            out,
            "{:<26} {} {:>5} {} {} {}   {:>14} {} {:>11}{note}",
            f.name,
            number(f.score, 6, 3),
            turn,
            number(f.crop_gap.map(|g| g * 100.0), 7, 1) + "%",
            number(f.median, 6, 1),
            number(f.p95, 6, 1),
            cast,
            number(f.chroma, 7, 1),
            f.edge_cast
                .map_or("-".to_string(), |e| format!("{:+.0} {:+.0}", e[0], e[1]))
        )?;
    }
    if report.rolls.len() > 1 {
        writeln!(out, "Per roll (all pixels of its compared frames):")?;
        for roll in &report.rolls {
            writeln!(
                out,
                "  {:<24} {} of {} compared: median {:.1}, p95 {:.1}",
                roll.roll, roll.compared, roll.frames, roll.median, roll.p95
            )?;
        }
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

/// How a set of frames fared, for the fit's table.
fn describe_score(score: Option<Score>) -> String {
    score.map_or("-".to_string(), |s| {
        format!(
            "median {:>5.1}, mean {:>5.1}, chroma {:>+5.1}",
            s.median_of_medians, s.mean, s.chroma
        )
    })
}

/// The fit as the command prints it.
fn print_fit(fit: &FitReport, out: &mut impl Write) -> std::io::Result<()> {
    writeln!(
        out,
        "Fitted on {} frames ({}), held out {} ({}).",
        fit.train.len(),
        fit.train.join(", "),
        fit.test.len(),
        fit.test.join(", ")
    )?;
    if !fit.excluded.is_empty() {
        writeln!(
            out,
            "Left out, not registered: {}.",
            fit.excluded.join(", ")
        )?;
    }
    writeln!(
        out,
        "ΔE2000: the median of the frames' medians, the mean over all pixels (each capped at {}), and our mean chroma minus the reference's.",
        fit::DE_CAP
    )?;
    for r in &fit.settings {
        writeln!(
            out,
            "\nLevels at {} and {} percent:",
            r.percentiles.low, r.percentiles.high
        )?;
        writeln!(
            out,
            "  neutral   fitted frames: {:<42} held out: {}",
            describe_score(Some(r.neutral_train)),
            describe_score(r.neutral_test)
        )?;
        writeln!(
            out,
            "  Frontier  fitted frames: {:<42} held out: {}",
            describe_score(Some(r.fitted_train)),
            describe_score(r.fitted_test)
        )?;
        writeln!(
            out,
            "  slope {:.3}, pivot {:.3}, gamma {:.3}, saturation {:.3}",
            r.params.slope, r.params.pivot, r.params.gamma, r.params.saturation
        )?;
    }
    Ok(())
}

fn run() -> Result<bool, Box<dyn Error>> {
    match parse(std::env::args().skip(1))? {
        Command::Manifest {
            scans,
            frames,
            references,
            out,
            format,
        } => {
            let (text, note) = if scans {
                manifest::build_scans(&frames, &references, &format)?
            } else {
                manifest::build(&frames, &references, &format)?
            };
            std::fs::write(&out, text)?;
            writeln!(std::io::stdout().lock(), "{note}\nWrote {}", out.display())?;
            Ok(true)
        }
        Command::Fit {
            manifest,
            holdout,
            percentiles,
        } => {
            let with_details = |err: gtp_core::AppError| {
                format!("{}: {}", err.message, err.details.unwrap_or_default())
            };
            let manifest = manifest::read(&manifest).map_err(with_details)?;
            let settings: Vec<LevelsPercentiles> = if percentiles.is_empty() {
                [(0.1, 99.9), (0.5, 99.5), (1.0, 99.0), (2.0, 98.0)]
                    .into_iter()
                    .collect::<Vec<_>>()
            } else {
                percentiles
            }
            .into_iter()
            .map(|(low, high)| LevelsPercentiles { low, high })
            .collect();
            let mut note = |line: &str| {
                let _ = writeln!(std::io::stderr(), "{line}");
            };
            let fit = fit::run(&manifest, holdout, &settings, &mut note).map_err(with_details)?;
            print_fit(&fit, &mut std::io::stdout().lock())?;
            Ok(true)
        }
        Command::Report {
            manifest,
            maps,
            look,
            balance,
        } => {
            // The details name the pair and the file, which the message alone does not.
            let with_details = |err: gtp_core::AppError| {
                format!("{}: {}", err.message, err.details.unwrap_or_default())
            };
            let manifest = manifest::read(&manifest).map_err(with_details)?;
            let mut note = |line: &str| {
                let _ = writeln!(std::io::stderr(), "{line}");
            };
            let report = report::run(&manifest, maps.as_deref(), look, balance, &mut note)
                .map_err(with_details)?;
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
        let Ok(Command::Report {
            manifest,
            maps,
            look,
            balance,
        }) = args(&["report", "--manifest", "m.json", "--maps", "out"])
        else {
            unreachable!("report should parse");
        };
        assert_eq!(
            (manifest, maps, look, balance),
            (
                PathBuf::from("m.json"),
                Some(PathBuf::from("out")),
                Look::Neutral,
                Balance::Frame(BalanceMethod::Off)
            )
        );
        let Ok(Command::Report { balance, .. }) =
            args(&["report", "--manifest", "m", "--balance", "near_neutral"])
        else {
            unreachable!("report with a balance should parse");
        };
        assert_eq!(balance, Balance::Frame(BalanceMethod::NearNeutral));
        assert!(args(&["report", "--manifest", "m", "--balance", "magic"]).is_err());
        assert!(args(&["report"]).is_err(), "a manifest is required");
        let Ok(Command::Report { look, .. }) =
            args(&["report", "--manifest", "m", "--look", "frontier"])
        else {
            unreachable!("report with a look should parse");
        };
        assert_eq!(look, Look::Frontier);
        assert!(args(&["report", "--manifest", "m", "--look", "sepia"]).is_err());

        // The fit holds out every third frame unless told, and takes any number of percentile pairs.
        let Ok(Command::Fit {
            holdout,
            percentiles,
            ..
        }) = args(&["fit", "--manifest", "m"])
        else {
            unreachable!("fit should parse");
        };
        assert_eq!((holdout, percentiles.len()), (3, 0));
        let Ok(Command::Fit {
            holdout,
            percentiles,
            ..
        }) = args(&[
            "fit",
            "--manifest",
            "m",
            "--holdout",
            "5",
            "--percentiles",
            "0.5,99.5",
            "--percentiles",
            "1,99",
        ])
        else {
            unreachable!("fit with options should parse");
        };
        assert_eq!(holdout, 5);
        assert_eq!(percentiles, vec![(0.5, 99.5), (1.0, 99.0)]);
        assert!(args(&["fit", "--manifest", "m", "--percentiles", "99,1"]).is_err());
        assert!(args(&["fit", "--manifest", "m", "--percentiles", "nope"]).is_err());
        assert!(args(&["fit"]).is_err());
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

        // LS-50 scans are 35 mm unless said; folders of both kinds at once are refused.
        let Ok(Command::Manifest { scans, format, .. }) = args(&[
            "manifest",
            "--scans",
            "s",
            "--references",
            "r",
            "--out",
            "m.json",
        ]) else {
            unreachable!("manifest of scans should parse");
        };
        assert!(scans);
        assert_eq!(format, "35mm");
        let Ok(Command::Manifest { scans, .. }) = args(&[
            "manifest",
            "--triplets",
            "t",
            "--references",
            "r",
            "--out",
            "m.json",
        ]) else {
            unreachable!("manifest of triplets should parse");
        };
        assert!(!scans);
        assert!(args(&[
            "manifest",
            "--triplets",
            "t",
            "--scans",
            "s",
            "--references",
            "r",
            "--out",
            "m.json"
        ])
        .is_err());
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
