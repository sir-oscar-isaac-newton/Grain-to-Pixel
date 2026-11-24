//! A frame's saved recipe fitted to the film type of its roll (P-36, ADR 0017).
//!
//! A frame of a black-and-white roll is inverted from one combined density (`invert.combine` set);
//! a frame of a colour roll is inverted per channel (`combine` empty). The film type of a roll can
//! change after its frames were analyzed, and a recipe can be copied from a frame of the other
//! kind, so the two can disagree. Wherever a frame's saved recipe is loaded to be shown, rendered
//! for a thumbnail or exported ([`fit`]), the recipe is fitted to the roll first:
//!
//! - black-and-white roll, `combine` empty: the combined inversion is found from the frame's own
//!   scan (weights and one pair of points), and `mono` is turned on;
//! - colour roll, `combine` set: `combine` is cleared, the per-channel points are found again, and
//!   `mono` is turned off;
//!
//! and every other edit is kept. The points the recipe held are not carried over: they were set
//! against one set of densities and mean nothing against the other (ADR 0017). The fitted recipe is
//! saved at once, without counting as an edit by the user, so the next load finds that it fits and
//! changes nothing.

use gtp_core::{AppError, FilmType, FrameId, Recipe};
use gtp_negative::{
    auto_levels, auto_levels_combined, DEFAULT_HIGH_PERCENTILE, DEFAULT_LOW_PERCENTILE,
};

use crate::pipeline::{normalized_frame, PipelineInput};
use crate::services::library::Library;

/// True when `recipe` is the kind `film_type` wants: combined for black and white, per channel for
/// colour.
pub(crate) fn fits(recipe: &Recipe, film_type: FilmType) -> bool {
    film_type.is_black_and_white() == recipe.invert.combine.is_some()
}

/// `recipe` fitted to `film_type`, or `None` when it already fits. The levels are found on the
/// frame of `input` (a proxy is enough) with the recipe's own crop and film base; dust removal is
/// left out, as it is when a frame is analyzed.
pub(crate) fn adapt(
    recipe: &Recipe,
    film_type: FilmType,
    input: &PipelineInput,
) -> Result<Option<Recipe>, AppError> {
    if fits(recipe, film_type) {
        return Ok(None);
    }
    let input = PipelineInput {
        ir: None,
        ..input.clone()
    };
    let frame = normalized_frame(&input, recipe)?;
    let mut next = recipe.clone();
    if film_type.is_black_and_white() {
        next.invert =
            auto_levels_combined(&frame, DEFAULT_LOW_PERCENTILE, DEFAULT_HIGH_PERCENTILE)?;
        next.mono.enabled = true;
    } else {
        next.invert = auto_levels(&frame, DEFAULT_LOW_PERCENTILE, DEFAULT_HIGH_PERCENTILE)?;
        next.mono.enabled = false;
    }
    Ok(Some(next.clamped()))
}

/// The saved recipe of `frame`, fitted to `film_type` (see the module). `input` is asked for only
/// when the recipe does not fit, so a frame that fits costs nothing; a failure to read the scan is
/// returned. A recipe that cannot be fitted (its frame has nothing to measure) is logged and
/// returned as saved, so the frame still opens and can be put right by hand. A fitted recipe is
/// stored only while the stored one is still `saved`: an edit the user saved in the meantime is
/// never overwritten, and that edit is fitted again on its next load. A read-only library stores
/// nothing, and the fitted recipe is used for this load only.
pub(crate) fn fit(
    library: &Library,
    frame: FrameId,
    saved: Recipe,
    film_type: FilmType,
    input: impl FnOnce() -> Result<PipelineInput, AppError>,
) -> Result<Recipe, AppError> {
    if fits(&saved, film_type) {
        return Ok(saved);
    }
    let input = input()?;
    let next = match adapt(&saved, film_type, &input) {
        Ok(Some(next)) => next,
        Ok(None) => return Ok(saved),
        Err(error) => {
            tracing::warn!(%frame, ?film_type, code = ?error.code, details = ?error.details,
                "the recipe could not be fitted to the film type; it is used as saved");
            return Ok(saved);
        }
    };
    if !library.is_read_only() {
        let replaced = library.write(|c| c.replace_recipe_if_unchanged(frame, &saved, &next))?;
        if !replaced {
            tracing::debug!(%frame, "the recipe changed while it was fitted; not stored");
        }
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use gtp_core::testing::synthetic::{Params, Pattern};

    use super::*;
    use crate::services::roll_frame::{roll_update, RollInput};
    use crate::services::testkit::Fixture;

    /// A black-and-white picture (the same wedge in every channel) with noise.
    fn params() -> Params {
        Params {
            width: 280,
            height: 180,
            rebate: [20; 4],
            pattern: Pattern::Wedge,
            noise: 0.03,
            ..Params::default()
        }
    }

    fn set_film_type(t: &Fixture, film_type: FilmType) {
        roll_update(
            &t.library,
            t.roll,
            RollInput {
                name: "Roll".into(),
                film_type,
                ..RollInput::default()
            },
        )
        .unwrap();
    }

    /// A colour recipe with edits of every kind the fitting must keep.
    fn edited() -> Recipe {
        let mut r = Recipe::default();
        r.color.temperature = 30.0;
        r.color.saturation = 1.4;
        r.color.red_cyan = 12.0;
        r.tone.exposure_ev = 0.4;
        r.tone.vignette.amount = -20.0;
        r.noise.luminance = 10.0;
        r.geometry.rot90 = 1;
        r.invert.black = [0.9, 0.1, 0.5];
        r.invert.white = [2.0, 1.5, 2.5];
        r
    }

    #[test]
    fn a_recipe_fits_when_its_inversion_matches_the_film_type() {
        let colour = Recipe::default();
        let mut combined = Recipe::default();
        combined.invert.combine = Some([0.2, 0.5, 0.3]);
        assert!(fits(&colour, FilmType::ColorNegative));
        assert!(!fits(&colour, FilmType::BwSilver));
        assert!(!fits(&colour, FilmType::BwChromogenic));
        assert!(fits(&combined, FilmType::BwSilver));
        assert!(fits(&combined, FilmType::BwChromogenic));
        assert!(!fits(&combined, FilmType::ColorNegative));
    }

    #[test]
    fn a_colour_recipe_is_fitted_when_the_roll_turns_black_and_white_and_back() {
        let t = Fixture::new("film-recipe");
        let id = t.imported(&params(), &edited());
        assert!(!t.edited(id));

        // Colour roll: nothing changes.
        assert_eq!(t.service.open(&t.library, id).unwrap().recipe, edited());

        set_film_type(&t, FilmType::BwSilver);
        let bw = t.service.open(&t.library, id).unwrap().recipe;
        let weights = bw.invert.combine.expect("combined");
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(bw.mono.enabled);
        assert!(
            bw.invert.black[0] == bw.invert.black[1] && bw.invert.black[1] == bw.invert.black[2]
        );
        assert!(
            bw.invert.white[0] == bw.invert.white[1] && bw.invert.white[1] == bw.invert.white[2]
        );
        assert_ne!(
            bw.invert.black[0], 0.5,
            "the mean of the old points is not carried over"
        );
        assert!(bw.invert.white[0] > bw.invert.black[0]);
        // Every other edit is kept.
        let mut rest = bw.clone();
        rest.invert = edited().invert;
        rest.mono = edited().mono;
        assert_eq!(rest, edited());
        // It was saved, and the frame does not count as edited by the user.
        assert_eq!(t.stored(id), Some(bw.clone()));
        assert!(!t.edited(id));
        // Loading it again changes nothing.
        assert_eq!(t.service.open(&t.library, id).unwrap().recipe, bw);

        // And back to colour: per-channel points again, mono off, the same edits.
        set_film_type(&t, FilmType::ColorNegative);
        let colour = t.service.open(&t.library, id).unwrap().recipe;
        assert_eq!(colour.invert.combine, None);
        assert!(!colour.mono.enabled);
        assert!(colour.invert.black[0] != colour.invert.black[1]);
        assert!(colour.invert.white[0] > colour.invert.black[0]);
        let mut rest = colour.clone();
        rest.invert = edited().invert;
        assert_eq!(rest, edited());
        assert_eq!(t.stored(id), Some(colour));
        assert!(!t.edited(id));
    }

    #[test]
    fn a_recipe_that_fits_is_returned_as_it_is_without_looking_at_the_scan() {
        let t = Fixture::new("film-recipe-fits");
        let id = t.imported(&params(), &edited());
        let refused = || -> Result<PipelineInput, AppError> {
            Err(AppError::new(
                gtp_core::ErrorCode::Internal,
                "the scan must not be asked for",
            ))
        };
        let same = fit(&t.library, id, edited(), FilmType::ColorNegative, refused).unwrap();
        assert_eq!(same, edited());
        assert_eq!(t.stored(id), Some(edited()), "nothing was saved");

        // A recipe that does not fit and cannot be fitted is an error, and nothing is saved.
        let error = fit(&t.library, id, edited(), FilmType::BwSilver, refused).unwrap_err();
        assert_eq!(error.code, gtp_core::ErrorCode::Internal);
        assert_eq!(t.stored(id), Some(edited()));
    }

    fn input_of(params: &Params) -> PipelineInput {
        let s = gtp_core::testing::synthetic::negative(7, params).unwrap();
        PipelineInput {
            rgb: std::sync::Arc::new(s.image),
            ir: None,
            film_base: None,
            full_long_edge: None,
        }
    }

    #[test]
    fn an_edit_saved_while_the_recipe_is_fitted_is_never_overwritten() {
        let t = Fixture::new("film-recipe-race");
        let id = t.imported(&params(), &edited());
        set_film_type(&t, FilmType::BwSilver);
        // The user saves an edit after the recipe was read and before the fitted one is stored.
        let mut theirs = edited();
        theirs.tone.exposure_ev = -0.8;
        let fitted = fit(&t.library, id, edited(), FilmType::BwSilver, || {
            crate::services::develop::save_recipe(&t.library, id, theirs.clone())?;
            Ok(input_of(&params()))
        })
        .unwrap();
        assert!(fitted.invert.combine.is_some(), "this load is still fitted");
        assert_eq!(
            t.stored(id),
            Some(theirs.clone()),
            "the user's edit is kept"
        );

        // The next load fits the user's edit and stores it.
        let opened = t.service.open(&t.library, id).unwrap().recipe;
        assert!(opened.invert.combine.is_some());
        assert_eq!(opened.tone.exposure_ev, -0.8);
        assert_eq!(t.stored(id), Some(opened));
    }

    #[test]
    fn a_recipe_that_cannot_be_fitted_is_used_as_saved_and_not_stored() {
        let t = Fixture::new("film-recipe-unfit");
        let id = t.imported(&params(), &edited());
        // A picture the inversion cannot measure: one channel instead of three.
        let image = gtp_core::ImageF32::new(64, 48, 1, vec![0.5; 64 * 48]).unwrap();
        let blank = PipelineInput {
            rgb: std::sync::Arc::new(image),
            ir: None,
            film_base: None,
            full_long_edge: None,
        };
        let mut no_crop = edited();
        no_crop.geometry = gtp_core::recipe::Geometry::default();
        let used = fit(&t.library, id, no_crop.clone(), FilmType::BwSilver, || {
            Ok(blank)
        })
        .unwrap();
        assert_eq!(used, no_crop);
        assert_eq!(t.stored(id), Some(edited()), "nothing was stored");
    }

    #[test]
    fn an_edited_frame_stays_edited_and_a_missing_recipe_is_not_made_up() {
        let t = Fixture::new("film-recipe-edited");
        let id = t.frame_params(1, &params());
        crate::services::develop::save_recipe(&t.library, id, edited()).unwrap();
        assert!(t.edited(id));
        set_film_type(&t, FilmType::BwChromogenic);
        let bw = t.service.open(&t.library, id).unwrap().recipe;
        assert!(bw.invert.combine.is_some());
        assert!(t.edited(id));

        // A frame that was never given a recipe still opens with the default one.
        let bare = t.frame_params(2, &params());
        let opened = t.service.open(&t.library, bare).unwrap().recipe;
        assert_eq!(opened, Recipe::default());
        assert_eq!(t.stored(bare), None);
    }
}
