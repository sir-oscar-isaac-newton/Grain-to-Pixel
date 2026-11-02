//! Automatic analysis of a frame: the values `render` will not guess by itself.

use std::sync::Arc;

use gtp_core::recipe::Geometry;
use gtp_core::{AppError, FilmFormat, Recipe, Roll};
use gtp_negative::{
    auto_film_base, auto_levels, detect_frame_with, normalize, BaseEstimate, FrameDetection,
    FrameHint, DEFAULT_HIGH_PERCENTILE, DEFAULT_LOW_PERCENTILE,
};

use super::{build_proxy, histogram, render, to_rgba8_srgb, Histogram, PipelineInput};
use super::{RenderTarget, PREVIEW_EDGE};

/// What [`analyze`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// The detected frame and its confidence (the full frame when not confident).
    pub frame: FrameDetection,
    /// The film base used: the roll's (confidence 1) or measured on this scan.
    pub base: BaseEstimate,
    /// True when the base came from the roll.
    pub base_from_roll: bool,
    /// Histogram of the auto recipe's render, in display values.
    pub histogram: Histogram,
    /// A recipe with the automatic crop, base and levels, everything else at its default.
    pub auto_recipe: Recipe,
}

/// Analyzes a scan on a [`PREVIEW_EDGE`] proxy: film base (the roll's, or measured), frame
/// detection on the base-normalized scan, auto levels on the cropped frame, then a render of the
/// resulting recipe for the histogram.
pub fn analyze(input: &PipelineInput) -> Result<Analysis, AppError> {
    analyze_with(input, None)
}

/// What frame detection may assume about a roll's frames: its format's aspect ratio and, once known,
/// the size of its frames. `None` for a 35 mm roll without a frame size, which is detected by its
/// rebate alone, as scans are.
pub fn frame_hint(roll: &Roll) -> Option<FrameHint> {
    (roll.format != FilmFormat::Mm35 || roll.frame_size.is_some()).then(|| FrameHint {
        aspect: roll.format.aspect(),
        size: roll.frame_size.map(|s| [s.w, s.h]),
    })
}

/// [`analyze`] with what the roll says about its frames (see [`frame_hint`]).
pub fn analyze_with(input: &PipelineInput, hint: Option<FrameHint>) -> Result<Analysis, AppError> {
    let proxy = build_proxy(&input.rgb, PREVIEW_EDGE)?;
    let (base, base_from_roll) = match input.film_base {
        Some(rgb) => (
            BaseEstimate {
                rgb,
                confidence: 1.0,
            },
            true,
        ),
        None => (auto_film_base(&proxy)?, false),
    };

    let mut normalized = proxy.clone();
    normalize(&mut normalized, base.rgb)?;
    let frame = detect_frame_with(&normalized, hint)?;
    let crop_only = Geometry {
        crop: frame.rect,
        ..Geometry::default()
    };
    let cropped = gtp_edit::apply_geometry(&normalized, &crop_only)?;
    let levels = auto_levels(&cropped, DEFAULT_LOW_PERCENTILE, DEFAULT_HIGH_PERCENTILE)?;

    let mut auto_recipe = Recipe::default();
    auto_recipe.geometry.crop = frame.rect;
    auto_recipe.base.rgb = base.rgb;
    auto_recipe.invert = levels;
    let auto_recipe = auto_recipe.clamped();

    let proxy_input = PipelineInput {
        rgb: Arc::new(proxy),
        ir: None,
        film_base: input.film_base,
    };
    let preview = render(&proxy_input, &auto_recipe, RenderTarget::Full)?;
    let histogram = histogram(&to_rgba8_srgb(&preview)?);

    Ok(Analysis {
        frame,
        base,
        base_from_roll,
        histogram,
        auto_recipe,
    })
}

#[cfg(test)]
mod tests {
    use gtp_core::testing::synthetic::{self, Params, Pattern, Synthetic};
    use gtp_core::ErrorCode;
    use gtp_core::ImageF32;

    use super::*;

    fn scan(width: u32, height: u32, rebate: u32) -> Synthetic {
        synthetic::negative(
            8,
            &Params {
                width,
                height,
                rebate: [rebate; 4],
                pattern: Pattern::Wedge,
                ..Params::default()
            },
        )
        .unwrap()
    }

    fn input_of(s: &Synthetic, film_base: Option<[f32; 3]>) -> PipelineInput {
        PipelineInput {
            rgb: Arc::new(s.image.clone()),
            ir: None,
            film_base,
        }
    }

    fn assert_frame_matches(s: &Synthetic, a: &Analysis) {
        let (w, h) = (s.image.width as f32, s.image.height as f32);
        let f = s.truth.frame;
        let want = [
            f.x as f32 / w,
            f.y as f32 / h,
            (f.x + f.w) as f32 / w,
            (f.y + f.h) as f32 / h,
        ];
        let r = a.frame.rect;
        let got = [r.x, r.y, r.x + r.w, r.y + r.h];
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() <= 0.005, "frame edge {g} vs {w}");
        }
    }

    /// A 300 x 200 capture of a 645 frame between opaque black margins: unexposed film from
    /// column 40 to 111, the picture from 111 to 260, black beyond.
    fn camera_capture(base: [f32; 3]) -> ImageF32 {
        let mut data = Vec::new();
        for y in 0..200usize {
            for x in 0..300usize {
                let v = if !(40..260).contains(&x) {
                    0.003
                } else if x >= 111 {
                    0.3 + 0.4 * ((x * 7 + y * 3) % 10) as f32 / 10.0
                } else {
                    1.0
                };
                data.extend(base.map(|b| v * b));
            }
        }
        ImageF32::new(300, 200, 3, data).unwrap()
    }

    #[test]
    fn a_roll_gives_a_hint_unless_it_is_35mm_without_a_frame_size() {
        use gtp_core::FrameSize;
        let mut roll = Roll {
            id: gtp_core::RollId::new(),
            name: "R".into(),
            film_stock: None,
            format: FilmFormat::Mm35,
            camera: None,
            iso_box: None,
            iso_shot: None,
            shot_from: None,
            shot_to: None,
            notes: String::new(),
            film_base_rgb: None,
            film_base_confidence: None,
            frame_size: None,
            created_at: String::new(),
        };
        assert_eq!(frame_hint(&roll), None);
        roll.format = FilmFormat::Mf645;
        let hint = frame_hint(&roll).unwrap();
        assert!((hint.aspect - 1.35).abs() < 0.02 && hint.size.is_none());
        roll.frame_size = Some(FrameSize {
            w: 0.5,
            h: 1.0,
            samples: 3,
        });
        assert_eq!(frame_hint(&roll).unwrap().size, Some([0.5, 1.0]));
        roll.format = FilmFormat::Mm35;
        assert!(frame_hint(&roll).is_some());
    }

    #[test]
    fn a_medium_format_hint_crops_a_camera_capture_that_has_no_usable_rebate() {
        let base = [0.8, 0.5, 0.3];
        let input = PipelineInput {
            rgb: Arc::new(camera_capture(base)),
            ir: None,
            film_base: Some(base),
        };
        let plain = analyze(&input).unwrap();
        assert!(plain.frame.confidence < gtp_negative::MIN_CONFIDENCE);

        let hint = FrameHint {
            aspect: FilmFormat::Mf645.aspect(),
            size: None,
        };
        let a = analyze_with(&input, Some(hint)).unwrap();
        assert!(a.frame.confidence >= gtp_negative::MIN_CONFIDENCE);
        let r = a.frame.rect;
        assert!((r.x - 111.0 / 300.0).abs() < 0.02, "{r:?}");
        assert!(a.frame.measured_size.is_none());
        // The levels now come from the picture, so its darkest and brightest values are used.
        assert!(a.auto_recipe.geometry.crop == a.frame.rect);
    }

    #[test]
    fn analysis_finds_the_frame_base_and_levels_and_its_recipe_renders() {
        // Larger than the analysis proxy, so the proxy path is exercised too.
        let s = scan(2400, 1600, 100);
        let a = analyze(&input_of(&s, None)).unwrap();
        assert!(!a.base_from_roll);
        for c in 0..3 {
            assert!(
                (a.base.rgb[c] - s.truth.base[c]).abs() / s.truth.base[c] < 0.01,
                "base {c}"
            );
            // On a wedge the percentiles are the true end points.
            assert!(
                (a.auto_recipe.invert.black[c] - s.truth.black[c]).abs() < 0.02,
                "black {c}"
            );
            assert!(
                (a.auto_recipe.invert.white[c] - s.truth.white[c]).abs() < 0.02,
                "white {c}"
            );
        }
        assert!(a.frame.confidence > 0.8);
        assert_frame_matches(&s, &a);
        assert_eq!(a.auto_recipe.geometry.crop, a.frame.rect);

        let pixels: u32 = a.histogram.luma.iter().sum();
        assert!(pixels > 0);
        assert_eq!(pixels, a.histogram.red.iter().sum::<u32>());
        render(&input_of(&s, None), &a.auto_recipe, RenderTarget::Full).unwrap();
    }

    #[test]
    fn a_roll_base_is_used_as_given() {
        let s = scan(600, 400, 40);
        let roll = s.truth.base;
        let a = analyze(&input_of(&s, Some(roll))).unwrap();
        assert!(a.base_from_roll);
        assert_eq!(a.base.rgb, roll);
        assert_eq!(a.base.confidence, 1.0);
        assert_frame_matches(&s, &a);
    }

    #[test]
    fn a_non_rgb_scan_is_rejected() {
        let gray = PipelineInput {
            rgb: Arc::new(ImageF32::zeros(8, 8, 1).unwrap()),
            ir: None,
            film_base: None,
        };
        assert_eq!(analyze(&gray).unwrap_err().code, ErrorCode::InvalidInput);
    }

    /// Analysis of a 21 MP scan (proxy, base, frame, levels, render, histogram); run with
    /// `cargo test -p gtp-app --release -- --ignored --nocapture` to see the time.
    #[test]
    #[ignore = "timing run, not a check"]
    fn time_analysis_of_a_21_megapixel_scan() {
        // The generator stops at 4096 px a side, so each pixel of a 2808 x 1872 negative is
        // doubled in both directions to reach 5616 x 3744 (21 MP) with the same layout.
        let s = scan(2808, 1872, 100);
        let (w, h) = (s.image.width as usize, s.image.height as usize);
        let mut data = Vec::with_capacity(w * h * 12);
        for y in 0..h * 2 {
            for x in 0..w * 2 {
                let at = ((y / 2) * w + x / 2) * 3;
                data.extend_from_slice(&s.image.data[at..at + 3]);
            }
        }
        let big = ImageF32::new(w as u32 * 2, h as u32 * 2, 3, data).unwrap();
        let input = PipelineInput {
            rgb: Arc::new(big),
            ir: None,
            film_base: None,
        };
        let start = std::time::Instant::now();
        let a = analyze(&input).unwrap();
        eprintln!(
            "analysis of 21 MP: {:?} (confidence {:.2})",
            start.elapsed(),
            a.frame.confidence
        );
    }
}
