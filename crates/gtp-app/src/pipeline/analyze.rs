//! Automatic analysis of a frame: the values `render` will not guess by itself.

use std::sync::Arc;

use gtp_core::recipe::Geometry;
use gtp_core::{AppError, Recipe};
use gtp_negative::{
    auto_film_base, auto_levels, detect_frame, normalize, BaseEstimate, FrameDetection,
    DEFAULT_HIGH_PERCENTILE, DEFAULT_LOW_PERCENTILE,
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
    let frame = detect_frame(&normalized)?;
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
