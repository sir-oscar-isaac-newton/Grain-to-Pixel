//! The automatic colour balance of a roll of scanner frames (P-22, decision 0011).
//!
//! A scan or import estimates each new frame's mid-tone neutral when its starting recipe is made
//! ([`crate::services::import::analyse_for_import`]). The roll gets **one** correction, the median
//! of those estimates (`gtp_edit::roll_balance`), which is known only when the job ends:
//!
//! - a roll that already has a stored balance gives it to every new frame at once;
//! - otherwise [`JobBalance::finish`] stores the median on the roll (the first stored stays, like
//!   the film base) and writes it into the starting recipes of the job's frames that the user has
//!   not edited, whose thumbnails are then made again;
//! - a job in which no frame could be judged stores nothing and says so (`BalanceSkipped`).
//!
//! Camera triplets never take part: their estimates raised the cast against the references.

use gtp_core::{
    AppError, FrameId, FrameKind, ImportResult, ImportWarning, ImportWarningKind, Recipe, RollId,
};

use crate::library_fs::thumb_rel;
use crate::pipeline::apply_balance_axes;
use crate::services::library::Library;

/// The frames one scan or import job stored on a roll, and what was estimated for them.
#[derive(Debug, Default)]
pub(crate) struct JobBalance {
    frames: Vec<FrameId>,
    estimates: Vec<[f32; 3]>,
}

/// What [`JobBalance::finish`] did.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Finished {
    /// Frames whose recipe took the roll's balance now; their thumbnails are out of date.
    pub rebalanced: Vec<FrameId>,
    /// The first frame of the job when none of its frames could be judged and the roll has no
    /// balance: the warning is about the job, and a warning names a frame.
    pub none_judged: Option<FrameId>,
}

impl JobBalance {
    /// Notes a stored frame and the axes estimated for it (`None` when it could not be judged, or
    /// was not estimated because the roll already had a balance).
    pub fn record(&mut self, frame: FrameId, estimate: Option<[f32; 3]>) {
        self.frames.push(frame);
        self.estimates.extend(estimate);
    }

    /// Stores the median of the estimates on the roll when it has none, and writes the roll's
    /// balance into the job's unedited frames that do not carry it yet. Runs after a cancelled job
    /// too, so the frames that were stored are consistent with the roll.
    pub fn finish(&self, library: &Library, roll: RollId) -> Result<Finished, AppError> {
        let Some(first) = self.frames.first().copied() else {
            return Ok(Finished::default());
        };
        let mut target = library.read(|c| c.get_roll(roll))?.color_balance;
        if target.is_none() {
            if let Some(median) = gtp_edit::roll_balance(&self.estimates) {
                target = library
                    .write(|c| c.store_roll_color_balance(roll, median))?
                    .0
                    .color_balance;
            }
        }
        let Some(axes) = target else {
            return Ok(Finished {
                rebalanced: Vec::new(),
                none_judged: Some(first),
            });
        };
        let rebalanced = library.write(|c| c.set_unedited_color_axes(&self.frames, axes))?;
        Ok(Finished {
            rebalanced,
            none_judged: None,
        })
    }
}

impl Finished {
    /// Says it in the job's result.
    pub fn report(self, result: &mut ImportResult) {
        if let Some(frame_id) = self.none_judged {
            result.warnings.push(ImportWarning {
                frame_id,
                kind: ImportWarningKind::BalanceSkipped,
            });
        }
        result.rebalanced = self.rebalanced;
    }

    /// For a job that ended early, whose result is lost: removes the out-of-date thumbnails, which
    /// are rebuilt from the saved recipe when they are asked for.
    pub fn drop_thumbnails(&self, library: &Library) {
        for frame in &self.rebalanced {
            if let Ok(path) = library.resolve(&thumb_rel(*frame)) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// The balance a frame's automatic recipe takes from its roll: that of the roll's scanner frames,
/// `None` for a camera triplet or a roll without one.
pub fn frame_balance(library: &Library, frame: FrameId) -> Result<Option<[f32; 3]>, AppError> {
    library.read(|c| {
        let frame = c.get_frame(frame)?;
        match frame.kind {
            FrameKind::ScanRgbIr => Ok(c.get_roll(frame.roll_id)?.color_balance),
            FrameKind::RgbTriplet => Ok(None),
        }
    })
}

/// `recipe`, an automatic one, with the roll's balance for `frame` written into its colour axes.
pub fn with_frame_balance(
    library: &Library,
    frame: FrameId,
    mut recipe: Recipe,
) -> Result<Recipe, AppError> {
    if let Some(axes) = frame_balance(library, frame)? {
        apply_balance_axes(&mut recipe, axes);
    }
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use gtp_core::{Frame, FrameKind, Recipe};

    use super::*;
    use crate::services::testkit::Fixture;

    fn add(t: &Fixture, index: u32, kind: FrameKind) -> FrameId {
        let id = FrameId::new();
        t.library
            .write(|c| {
                c.insert_imported_frame(
                    &Frame {
                        id,
                        roll_id: t.roll,
                        index_in_roll: index,
                        kind,
                        width: 10,
                        height: 10,
                        has_ir: false,
                        note: String::new(),
                        created_at: String::new(),
                        edited_at: None,
                    },
                    &[],
                    &Recipe::default(),
                )
            })
            .unwrap();
        id
    }

    fn axes(t: &Fixture, frame: FrameId) -> [f32; 3] {
        let r = t.stored(frame).unwrap();
        [r.color.red_cyan, r.color.green_magenta, r.color.blue_yellow]
    }

    #[test]
    fn the_median_is_stored_and_written_into_the_frames_the_user_has_not_edited() {
        let t = Fixture::new("balance-finish");
        let frames: Vec<FrameId> = (1..=4).map(|i| add(&t, i, FrameKind::ScanRgbIr)).collect();
        // The user edits frame 2 while the job still runs.
        t.library
            .write(|c| c.save_recipe(frames[1], &Recipe::default()))
            .unwrap();
        let mut job = JobBalance::default();
        job.record(frames[0], Some([1.0, 0.0, 6.0]));
        job.record(frames[1], Some([2.0, 4.0, 0.0]));
        job.record(frames[2], None);
        job.record(frames[3], Some([9.0, -4.0, 3.0]));
        let finished = job.finish(&t.library, t.roll).unwrap();

        let median = [2.0, 0.0, 3.0];
        let roll = t.library.read(|c| c.get_roll(t.roll)).unwrap();
        assert_eq!(roll.color_balance, Some(median));
        assert_eq!(finished.none_judged, None);
        assert_eq!(finished.rebalanced, vec![frames[0], frames[2], frames[3]]);
        for i in [0, 2, 3] {
            assert_eq!(axes(&t, frames[i]), median, "frame {i}, judged or not");
            assert!(!t.edited(frames[i]), "the balance is not an edit");
        }
        assert_eq!(
            axes(&t, frames[1]),
            [0.0; 3],
            "an edited frame is left alone"
        );
    }

    #[test]
    fn a_stored_balance_stays_and_the_job_follows_it() {
        let t = Fixture::new("balance-kept");
        t.library
            .write(|c| c.store_roll_color_balance(t.roll, [5.0, 5.0, 5.0]))
            .unwrap();
        let frame = add(&t, 1, FrameKind::ScanRgbIr);
        let mut job = JobBalance::default();
        job.record(frame, Some([-8.0, 1.0, 2.0]));
        let finished = job.finish(&t.library, t.roll).unwrap();
        let roll = t.library.read(|c| c.get_roll(t.roll)).unwrap();
        assert_eq!(roll.color_balance, Some([5.0, 5.0, 5.0]), "the first stays");
        assert_eq!(axes(&t, frame), [5.0, 5.0, 5.0]);
        assert_eq!(finished.rebalanced, vec![frame]);
    }

    #[test]
    fn a_job_without_a_judgeable_frame_stores_nothing_and_warns_on_its_first_frame() {
        let t = Fixture::new("balance-none");
        let frames: Vec<FrameId> = (1..=2).map(|i| add(&t, i, FrameKind::ScanRgbIr)).collect();
        let mut job = JobBalance::default();
        for f in &frames {
            job.record(*f, None);
        }
        let finished = job.finish(&t.library, t.roll).unwrap();
        assert!(t
            .library
            .read(|c| c.get_roll(t.roll))
            .unwrap()
            .color_balance
            .is_none());
        assert_eq!(axes(&t, frames[0]), [0.0; 3]);
        let mut result = ImportResult::default();
        finished.report(&mut result);
        assert_eq!(
            result.warnings,
            vec![ImportWarning {
                frame_id: frames[0],
                kind: ImportWarningKind::BalanceSkipped
            }]
        );
        assert!(result.rebalanced.is_empty());

        // A job that stored no frame has nothing to say.
        let none = JobBalance::default().finish(&t.library, t.roll).unwrap();
        assert_eq!(none, Finished::default());
    }

    #[test]
    fn only_scanner_frames_take_the_rolls_balance_into_an_automatic_recipe() {
        let t = Fixture::new("balance-kinds");
        let scan = add(&t, 1, FrameKind::ScanRgbIr);
        let triplet = add(&t, 2, FrameKind::RgbTriplet);
        assert_eq!(
            frame_balance(&t.library, scan).unwrap(),
            None,
            "none stored"
        );
        t.library
            .write(|c| c.store_roll_color_balance(t.roll, [3.0, -2.0, 1.0]))
            .unwrap();
        assert_eq!(
            frame_balance(&t.library, scan).unwrap(),
            Some([3.0, -2.0, 1.0])
        );
        assert_eq!(frame_balance(&t.library, triplet).unwrap(), None);
        let auto = with_frame_balance(&t.library, scan, Recipe::default()).unwrap();
        assert_eq!(auto.color.red_cyan, 3.0);
        assert_eq!(auto.color.green_magenta, -2.0);
        assert_eq!(auto.color.blue_yellow, 1.0);
        let auto = with_frame_balance(&t.library, triplet, Recipe::default()).unwrap();
        assert_eq!(auto, Recipe::default());
    }
}
