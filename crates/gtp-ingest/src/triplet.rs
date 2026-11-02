//! Grouping camera files into R/G/B triplets (specification section 6, `triplet_group`).
//!
//! A triplet is three exposures of one frame, lit red, then green, then blue. The camera does not
//! say which is which, so the files are put in capture order and the first of each three is taken
//! as red. The user can reorder a candidate when that is wrong.
//!
//! Shots of one triplet follow each other within seconds (2 to 3 s on a D600 with the Scanlight);
//! moving to the next frame takes longer (8 s and more). A pause longer than [`MAX_SHOT_GAP_MS`]
//! therefore ends a run of shots. A run of three is a triplet; any other run is returned as one
//! flagged candidate so the user can see what is missing or extra.

use std::path::PathBuf;

use serde::Serialize;

/// A pause between two shots longer than this (milliseconds) starts a new triplet.
pub const MAX_SHOT_GAP_MS: i64 = 5_000;

/// When a file was shot, as the camera wrote it: local time without a time zone, in milliseconds
/// since 1970-01-01 00:00 as if that time were UTC. Only differences between files of one camera
/// mean anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CaptureTime(pub i64);

/// A camera file with what grouping needs from its EXIF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExifFile {
    pub path: PathBuf,
    /// `None` when the file has no readable capture time.
    pub captured: Option<CaptureTime>,
}

/// A file of a candidate, for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TripletFile {
    pub path: String,
    /// The capture time as `YYYY-MM-DDTHH:MM:SS.mmm` (no time zone), if the file has one.
    pub captured: Option<String>,
}

/// What is wrong with a candidate; a candidate without a problem is a complete triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TripletProblem {
    /// The run has fewer shots than a whole number of triplets: `missing` more are needed.
    MissingShots { missing: u32 },
    /// The run has more shots than a whole number of triplets: `extra` too many.
    ExtraShots { extra: u32 },
    /// The files have no capture time, so they could not be put in order or grouped.
    NoCaptureTime,
}

/// Files that probably belong together. With no `problem` there are exactly three, red first, then
/// green, then blue. With a problem they are in capture order (by file name when the time is
/// missing) and the user decides which to use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TripletCandidate {
    pub files: Vec<TripletFile>,
    pub problem: Option<TripletProblem>,
}

impl TripletCandidate {
    /// The same files with the channels reassigned: position `i` of the result is the file at
    /// `order[i]` of this candidate. `None` unless `order` names every file exactly once.
    pub fn reordered(&self, order: &[usize]) -> Option<Self> {
        if order.len() != self.files.len() {
            return None;
        }
        let mut seen = vec![false; order.len()];
        for &at in order {
            if at >= seen.len() || std::mem::replace(&mut seen[at], true) {
                return None;
            }
        }
        Some(Self {
            files: order.iter().map(|&at| self.files[at].clone()).collect(),
            problem: self.problem,
        })
    }
}

fn describe(file: &ExifFile) -> TripletFile {
    TripletFile {
        path: file.path.display().to_string(),
        captured: file.captured.map(|time| format_time(time.0)),
    }
}

/// `YYYY-MM-DDTHH:MM:SS.mmm` for a [`CaptureTime`] value.
pub(crate) fn format_time(millis: i64) -> String {
    let days = millis.div_euclid(86_400_000);
    let in_day = millis.rem_euclid(86_400_000);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1000 % 60,
        in_day % 1000
    )
}

fn candidate(files: &[&ExifFile], problem: Option<TripletProblem>) -> TripletCandidate {
    TripletCandidate {
        files: files.iter().map(|file| describe(file)).collect(),
        problem,
    }
}

/// What a run of `shots` shots (not a whole number of triplets) is missing or has too many, the
/// smaller fix first: 4 is one extra, 5 is one missing; a run under three is short of three.
fn uneven(shots: usize) -> TripletProblem {
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    if shots < 3 {
        TripletProblem::MissingShots {
            missing: count(3 - shots),
        }
    } else if shots % 3 == 1 {
        TripletProblem::ExtraShots { extra: 1 }
    } else {
        TripletProblem::MissingShots { missing: 1 }
    }
}

/// Groups `files` into triplet candidates, in the order of their first shot (files without a
/// capture time last).
///
/// A run of shots ends at a pause longer than [`MAX_SHOT_GAP_MS`]. A run whose length is a
/// multiple of three is cut into triplets of three; any other run becomes one candidate flagged
/// with [`TripletProblem::MissingShots`] or [`TripletProblem::ExtraShots`]. All files without a
/// capture time form one candidate flagged [`TripletProblem::NoCaptureTime`].
pub fn group_triplets(files: &[ExifFile]) -> Vec<TripletCandidate> {
    let mut timed: Vec<(&ExifFile, CaptureTime)> = files
        .iter()
        .filter_map(|file| file.captured.map(|time| (file, time)))
        .collect();
    timed.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.path.cmp(&b.0.path)));

    let mut candidates = Vec::new();
    let mut run: Vec<&ExifFile> = Vec::new();
    let mut last: Option<CaptureTime> = None;
    let close = |run: &mut Vec<&ExifFile>, candidates: &mut Vec<TripletCandidate>| {
        if run.is_empty() {
            return;
        }
        if run.len().is_multiple_of(3) {
            candidates.extend(run.chunks(3).map(|three| candidate(three, None)));
        } else {
            candidates.push(candidate(run, Some(uneven(run.len()))));
        }
        run.clear();
    };
    for (file, time) in timed {
        if last.is_some_and(|before| time.0 - before.0 > MAX_SHOT_GAP_MS) {
            close(&mut run, &mut candidates);
        }
        run.push(file);
        last = Some(time);
    }
    close(&mut run, &mut candidates);

    let mut untimed: Vec<&ExifFile> = files.iter().filter(|f| f.captured.is_none()).collect();
    untimed.sort_by(|a, b| a.path.cmp(&b.path));
    if !untimed.is_empty() {
        candidates.push(candidate(&untimed, Some(TripletProblem::NoCaptureTime)));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file named `name` shot `seconds` after the start of the session.
    fn shot(name: &str, seconds: f64) -> ExifFile {
        ExifFile {
            path: PathBuf::from(name),
            captured: Some(CaptureTime(1_000_000 + (seconds * 1000.0) as i64)),
        }
    }

    fn untimed(name: &str) -> ExifFile {
        ExifFile {
            path: PathBuf::from(name),
            captured: None,
        }
    }

    fn names(candidate: &TripletCandidate) -> Vec<&str> {
        candidate.files.iter().map(|f| f.path.as_str()).collect()
    }

    /// The gaps of the real D600 session: 2 to 3 s inside a triplet, 8 s and more between.
    fn session(triplets: usize) -> Vec<ExifFile> {
        let between = [
            10.0, 13.0, 83.0, 16.0, 8.0, 11.0, 8.0, 13.0, 16.0, 10.0, 11.0,
        ];
        let within = [2.0, 2.5, 3.0, 2.0];
        let mut seconds = 0.0;
        let mut files = Vec::new();
        for t in 0..triplets {
            for c in 0..3 {
                files.push(shot(&format!("f{t:02}{c}.NEF"), seconds));
                seconds += within[(t + c) % within.len()];
            }
            seconds += between[t % between.len()];
        }
        files
    }

    #[test]
    fn shots_in_a_row_become_triplets_in_capture_order_red_green_blue() {
        let files = session(6);
        // Shuffled: the order of the input must not matter.
        let mut shuffled = files.clone();
        shuffled.reverse();
        shuffled.swap(2, 11);
        let candidates = group_triplets(&shuffled);
        assert_eq!(candidates.len(), 6);
        for (t, candidate) in candidates.iter().enumerate() {
            assert_eq!(candidate.problem, None, "{t}");
            assert_eq!(
                names(candidate),
                [
                    format!("f{t:02}0.NEF"),
                    format!("f{t:02}1.NEF"),
                    format!("f{t:02}2.NEF")
                ]
            );
        }
    }

    #[test]
    fn a_long_pause_ends_a_triplet_and_a_short_one_does_not() {
        let at_limit = [
            shot("a", 0.0),
            shot("b", 5.0),
            shot("c", 10.0),
            shot("d", 15.001),
            shot("e", 17.0),
            shot("f", 19.0),
        ];
        let candidates = group_triplets(&at_limit);
        assert_eq!(candidates.len(), 2);
        assert_eq!(names(&candidates[0]), ["a", "b", "c"]);
        assert_eq!(
            candidates[0].problem, None,
            "exactly the limit is still one run"
        );
        assert_eq!(names(&candidates[1]), ["d", "e", "f"]);
    }

    #[test]
    fn a_missing_shot_is_flagged_and_does_not_disturb_the_triplets_around_it() {
        let files = [
            shot("a1", 0.0),
            shot("a2", 2.0),
            shot("a3", 4.0),
            shot("b1", 20.0),
            shot("b2", 22.0),
            shot("c1", 40.0),
            shot("c2", 42.0),
            shot("c3", 44.0),
        ];
        let candidates = group_triplets(&files);
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].problem, None);
        assert_eq!(
            candidates[1].problem,
            Some(TripletProblem::MissingShots { missing: 1 })
        );
        assert_eq!(names(&candidates[1]), ["b1", "b2"]);
        assert_eq!(candidates[2].problem, None);
    }

    #[test]
    fn a_lone_shot_is_two_short_and_an_extra_shot_is_flagged() {
        let lone = group_triplets(&[shot("x", 0.0)]);
        assert_eq!(
            lone[0].problem,
            Some(TripletProblem::MissingShots { missing: 2 })
        );

        let four = group_triplets(&[
            shot("a", 0.0),
            shot("b", 2.0),
            shot("c", 4.0),
            shot("d", 6.0),
        ]);
        assert_eq!(four.len(), 1);
        assert_eq!(
            four[0].problem,
            Some(TripletProblem::ExtraShots { extra: 1 })
        );
        assert_eq!(
            names(&four[0]),
            ["a", "b", "c", "d"],
            "all of them, for the user to choose"
        );
    }

    #[test]
    fn uneven_runs_name_the_smaller_fix() {
        let run = |n: usize| {
            let files: Vec<_> = (0..n)
                .map(|i| shot(&format!("s{i:02}"), i as f64))
                .collect();
            group_triplets(&files)
        };
        let problem = |n: usize| run(n).last().and_then(|c| c.problem);
        assert_eq!(
            problem(2),
            Some(TripletProblem::MissingShots { missing: 1 })
        );
        assert_eq!(
            problem(5),
            Some(TripletProblem::MissingShots { missing: 1 })
        );
        assert_eq!(problem(7), Some(TripletProblem::ExtraShots { extra: 1 }));
        assert_eq!(
            problem(8),
            Some(TripletProblem::MissingShots { missing: 1 })
        );
        assert_eq!(
            run(6).len(),
            2,
            "a run of six shots in a row is two triplets"
        );
        assert!(run(9).iter().all(|c| c.problem.is_none()));
    }

    #[test]
    fn equal_times_are_ordered_by_file_name() {
        let files = [shot("c", 0.0), shot("a", 0.0), shot("b", 0.0)];
        assert_eq!(names(&group_triplets(&files)[0]), ["a", "b", "c"]);
    }

    #[test]
    fn files_without_a_capture_time_form_one_flagged_candidate_after_the_rest() {
        let files = [
            untimed("z.NEF"),
            shot("a", 0.0),
            untimed("m.NEF"),
            shot("b", 2.0),
            shot("c", 4.0),
        ];
        let candidates = group_triplets(&files);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].problem, None);
        assert_eq!(candidates[1].problem, Some(TripletProblem::NoCaptureTime));
        assert_eq!(names(&candidates[1]), ["m.NEF", "z.NEF"]);
        assert_eq!(candidates[1].files[0].captured, None);
        assert!(group_triplets(&[]).is_empty());
    }

    #[test]
    fn reordering_changes_which_file_is_red_green_and_blue() {
        let files = [shot("r", 0.0), shot("g", 2.0), shot("b", 4.0)];
        let candidate = group_triplets(&files).remove(0);
        assert_eq!(names(&candidate), ["r", "g", "b"]);

        let swapped = candidate.reordered(&[2, 1, 0]).unwrap();
        assert_eq!(names(&swapped), ["b", "g", "r"]);
        assert_eq!(swapped.problem, None);
        assert_eq!(
            swapped.reordered(&[2, 1, 0]).unwrap(),
            candidate,
            "undoes itself"
        );
        assert_eq!(
            names(&candidate.reordered(&[1, 2, 0]).unwrap()),
            ["g", "b", "r"]
        );
    }

    #[test]
    fn an_order_that_is_not_a_permutation_is_refused() {
        let candidate = group_triplets(&[shot("r", 0.0), shot("g", 2.0), shot("b", 4.0)]).remove(0);
        for bad in [&[0, 1][..], &[0, 1, 1], &[0, 1, 3], &[0, 1, 2, 0], &[]] {
            assert_eq!(candidate.reordered(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn capture_times_are_shown_as_dates_with_milliseconds() {
        // 2026-10-06 19:43:34.200, via days since the epoch.
        let days = 20_732;
        let millis = days * 86_400_000 + (19 * 3600 + 43 * 60 + 34) * 1000 + 200;
        assert_eq!(format_time(millis), "2026-10-06T19:43:34.200");
        assert_eq!(format_time(0), "1970-01-01T00:00:00.000");
        assert_eq!(
            format_time(951_782_400_000 + 86_399_999),
            "2000-02-29T23:59:59.999"
        );
    }
}
