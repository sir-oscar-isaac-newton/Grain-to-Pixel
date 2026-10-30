//! Range limits for every Recipe value. `Recipe::clamp` pulls each value into its range and
//! replaces NaN and infinite values by the default, so a damaged file cannot reach the pipeline.
//! Clamping is idempotent: clamping an already clamped recipe changes nothing.

use super::v1::{
    Color, Crop, CurvePoint, Dust, Geometry, Invert, Mono, Recipe, Tone, REC2020_LUMA,
};

/// Smallest crop width or height, as a fraction of the frame.
pub const MIN_CROP_SIZE: f32 = 0.01;
/// Straighten angle limit in degrees; larger turns use `rot90`.
pub const ANGLE_DEG_MAX: f32 = 45.0;
/// Film base values are linear scan values; the upper limit is the 16-bit maximum.
pub const BASE_MIN: f32 = 1e-6;
pub const BASE_MAX: f32 = 65535.0;
/// Black and white points are densities.
pub const INVERT_MAX: f32 = 10.0;
/// White stays at least this far above black.
pub const INVERT_GAP: f32 = 1e-3;
/// Limit of the colour and tone sliders (temperature, tint, CMY, contrast, highlights, ...).
pub const SLIDER_MAX: f32 = 100.0;
pub const SATURATION_MAX: f32 = 2.0;
pub const EXPOSURE_EV_MAX: f32 = 5.0;
pub const CURVE_POINTS_MAX: usize = 16;
pub const DUST_GROW_PX_MAX: u32 = 10;
/// Limit of each black-and-white mix weight.
pub const MONO_WEIGHT_MAX: f32 = 2.0;

fn fin(value: f32, lo: f32, hi: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(lo, hi)
    } else {
        default
    }
}

fn slider(value: f32) -> f32 {
    fin(value, -SLIDER_MAX, SLIDER_MAX, 0.0)
}

fn clamp_crop(crop: &mut Crop) {
    crop.w = fin(crop.w, MIN_CROP_SIZE, 1.0, 1.0);
    crop.h = fin(crop.h, MIN_CROP_SIZE, 1.0, 1.0);
    crop.x = fin(crop.x, 0.0, 1.0 - crop.w, 0.0);
    crop.y = fin(crop.y, 0.0, 1.0 - crop.h, 0.0);
}

fn clamp_geometry(geometry: &mut Geometry) {
    clamp_crop(&mut geometry.crop);
    geometry.angle_deg = fin(geometry.angle_deg, -ANGLE_DEG_MAX, ANGLE_DEG_MAX, 0.0);
    geometry.rot90 = geometry.rot90.rem_euclid(4);
}

fn clamp_invert(invert: &mut Invert) {
    for i in 0..3 {
        invert.black[i] = fin(invert.black[i], 0.0, INVERT_MAX - INVERT_GAP, 0.0);
        let white = fin(invert.white[i], 0.0, INVERT_MAX, 1.0);
        invert.white[i] = white.max(invert.black[i] + INVERT_GAP);
    }
}

fn clamp_mono(mono: &mut Mono) {
    for (weight, default) in mono.mix.iter_mut().zip(REC2020_LUMA) {
        *weight = fin(*weight, -MONO_WEIGHT_MAX, MONO_WEIGHT_MAX, default);
    }
}

fn clamp_color(color: &mut Color) {
    color.red_cyan = slider(color.red_cyan);
    color.green_magenta = slider(color.green_magenta);
    color.blue_yellow = slider(color.blue_yellow);
    color.temperature = slider(color.temperature);
    color.tint = slider(color.tint);
    color.saturation = fin(color.saturation, 0.0, SATURATION_MAX, 1.0);
    color.vibrance = slider(color.vibrance);
}

/// Points are limited to the unit square, sorted by input, made unique by input and capped.
fn clamp_curve(points: &mut Vec<CurvePoint>) {
    points.retain(|p| p[0].is_finite() && p[1].is_finite());
    for p in points.iter_mut() {
        p[0] = p[0].clamp(0.0, 1.0);
        p[1] = p[1].clamp(0.0, 1.0);
    }
    points.sort_by(|a, b| a[0].total_cmp(&b[0]));
    points.dedup_by(|later, earlier| later[0] == earlier[0]);
    points.truncate(CURVE_POINTS_MAX);
}

fn clamp_tone(tone: &mut Tone) {
    tone.exposure_ev = fin(tone.exposure_ev, -EXPOSURE_EV_MAX, EXPOSURE_EV_MAX, 0.0);
    tone.contrast = slider(tone.contrast);
    tone.highlights = slider(tone.highlights);
    tone.shadows = slider(tone.shadows);
    tone.whites = slider(tone.whites);
    tone.blacks = slider(tone.blacks);
    tone.dehaze = slider(tone.dehaze);
    clamp_curve(&mut tone.curve.master);
    clamp_curve(&mut tone.curve.r);
    clamp_curve(&mut tone.curve.g);
    clamp_curve(&mut tone.curve.b);
}

fn clamp_dust(dust: &mut Dust) {
    dust.threshold = fin(dust.threshold, 0.0, 1.0, 0.5);
    dust.grow_px = dust.grow_px.min(DUST_GROW_PX_MAX);
}

impl Recipe {
    /// Brings every value into its documented range (see the constants of this module).
    /// The version number `v` is not touched; `migrate` handles versions.
    pub fn clamp(&mut self) {
        clamp_geometry(&mut self.geometry);
        for channel in &mut self.base.rgb {
            *channel = fin(*channel, BASE_MIN, BASE_MAX, 1.0);
        }
        clamp_invert(&mut self.invert);
        clamp_color(&mut self.color);
        clamp_mono(&mut self.mono);
        clamp_tone(&mut self.tone);
        clamp_dust(&mut self.dust);
    }

    /// [`Recipe::clamp`] on a value.
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.clamp();
        self
    }
}

#[cfg(test)]
// Tests start from the default recipe and set only the field under test, which reads best.
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use proptest::array::{uniform2, uniform3};
    use proptest::collection::vec;
    use proptest::num::f32::ANY;
    use proptest::prelude::*;

    use super::*;

    /// A recipe whose floats can be anything, including NaN and infinities.
    fn any_recipe() -> impl Strategy<Value = Recipe> {
        let curve = || vec(uniform2(ANY), 0..40);
        (
            vec(ANY, 32),
            (curve(), curve(), curve(), curve()),
            (any::<i32>(), any::<u32>(), any::<bool>()),
            (uniform3(ANY), uniform3(ANY)),
        )
            .prop_map(
                |(f, (cm, cr, cg, cb), (rot90, grow_px, flag), (black, white))| {
                    let mut next = f.into_iter();
                    let mut n = move || next.next().unwrap_or(0.0);
                    let mut r = Recipe::default();
                    r.geometry.crop = Crop {
                        x: n(),
                        y: n(),
                        w: n(),
                        h: n(),
                    };
                    r.geometry.angle_deg = n();
                    r.geometry.rot90 = rot90;
                    r.geometry.flip_h = flag;
                    r.base.rgb = [n(), n(), n()];
                    r.invert = Invert { black, white };
                    r.color = Color {
                        red_cyan: n(),
                        green_magenta: n(),
                        blue_yellow: n(),
                        temperature: n(),
                        tint: n(),
                        saturation: n(),
                        vibrance: n(),
                    };
                    r.tone.exposure_ev = n();
                    r.tone.contrast = n();
                    r.tone.highlights = n();
                    r.tone.shadows = n();
                    r.tone.whites = n();
                    r.tone.blacks = n();
                    r.tone.dehaze = n();
                    r.mono = Mono {
                        enabled: flag,
                        mix: [n(), n(), n()],
                    };
                    r.tone.curve.master = cm;
                    r.tone.curve.r = cr;
                    r.tone.curve.g = cg;
                    r.tone.curve.b = cb;
                    r.dust = Dust {
                        enabled: flag,
                        threshold: n(),
                        grow_px,
                    };
                    r
                },
            )
    }

    fn in_range(value: f32, lo: f32, hi: f32) -> bool {
        value.is_finite() && value >= lo && value <= hi
    }

    fn curve_is_valid(points: &[CurvePoint]) -> bool {
        points.len() <= CURVE_POINTS_MAX
            && points
                .iter()
                .all(|p| in_range(p[0], 0.0, 1.0) && in_range(p[1], 0.0, 1.0))
            && points.windows(2).all(|w| w[0][0] < w[1][0])
    }

    /// Every documented range, checked on a clamped recipe.
    fn assert_all_ranges(r: &Recipe) {
        let c = r.geometry.crop;
        assert!(in_range(c.w, MIN_CROP_SIZE, 1.0) && in_range(c.h, MIN_CROP_SIZE, 1.0));
        assert!(in_range(c.x, 0.0, 1.0 - c.w) && in_range(c.y, 0.0, 1.0 - c.h));
        assert!(in_range(
            r.geometry.angle_deg,
            -ANGLE_DEG_MAX,
            ANGLE_DEG_MAX
        ));
        assert!((0..4).contains(&r.geometry.rot90));
        for v in r.base.rgb {
            assert!(in_range(v, BASE_MIN, BASE_MAX));
        }
        for i in 0..3 {
            assert!(in_range(r.invert.black[i], 0.0, INVERT_MAX));
            assert!(in_range(r.invert.white[i], 0.0, INVERT_MAX));
            assert!(r.invert.white[i] >= r.invert.black[i] + INVERT_GAP);
        }
        let k = r.color;
        for v in [
            k.red_cyan,
            k.green_magenta,
            k.blue_yellow,
            k.temperature,
            k.tint,
            k.vibrance,
        ] {
            assert!(in_range(v, -SLIDER_MAX, SLIDER_MAX));
        }
        assert!(in_range(k.saturation, 0.0, SATURATION_MAX));
        let t = &r.tone;
        assert!(in_range(t.exposure_ev, -EXPOSURE_EV_MAX, EXPOSURE_EV_MAX));
        for v in r.mono.mix {
            assert!(in_range(v, -MONO_WEIGHT_MAX, MONO_WEIGHT_MAX));
        }
        for v in [
            t.contrast,
            t.highlights,
            t.shadows,
            t.whites,
            t.blacks,
            t.dehaze,
        ] {
            assert!(in_range(v, -SLIDER_MAX, SLIDER_MAX));
        }
        for curve in [&t.curve.master, &t.curve.r, &t.curve.g, &t.curve.b] {
            assert!(curve_is_valid(curve));
        }
        assert!(in_range(r.dust.threshold, 0.0, 1.0));
        assert!(r.dust.grow_px <= DUST_GROW_PX_MAX);
    }

    proptest! {
        #[test]
        fn clamping_is_idempotent(recipe in any_recipe()) {
            let once = recipe.clamped();
            let twice = once.clone().clamped();
            prop_assert_eq!(once, twice);
        }

        #[test]
        fn a_clamped_recipe_is_inside_every_documented_range(recipe in any_recipe()) {
            assert_all_ranges(&recipe.clamped());
        }
    }

    #[test]
    fn the_default_recipe_is_already_in_range_and_unchanged_by_clamping() {
        let default = Recipe::default();
        assert_all_ranges(&default);
        assert_eq!(default.clone().clamped(), default);
    }

    #[test]
    fn out_of_range_sliders_stop_at_the_limits() {
        let mut r = Recipe::default();
        r.color.red_cyan = 900.0;
        r.color.blue_yellow = -900.0;
        r.tone.whites = f32::INFINITY;
        r.tone.shadows = f32::NAN;
        r.color.saturation = 7.0;
        r.clamp();
        assert_eq!(r.color.red_cyan, 100.0);
        assert_eq!(r.color.blue_yellow, -100.0);
        assert_eq!(
            r.tone.whites, 0.0,
            "non-finite values fall back to the default"
        );
        assert_eq!(r.tone.shadows, 0.0);
        assert_eq!(r.color.saturation, 2.0);
    }

    #[test]
    fn rot90_wraps_around() {
        let mut r = Recipe::default();
        r.geometry.rot90 = -1;
        assert_eq!(r.clamped().geometry.rot90, 3);
        let mut r = Recipe::default();
        r.geometry.rot90 = 6;
        assert_eq!(r.clamped().geometry.rot90, 2);
    }

    #[test]
    fn a_crop_cannot_leave_the_frame_or_collapse() {
        let mut r = Recipe::default();
        r.geometry.crop = Crop {
            x: 0.9,
            y: -3.0,
            w: 0.5,
            h: 0.0,
        };
        let crop = r.clamped().geometry.crop;
        assert_eq!(crop.w, 0.5);
        assert_eq!(crop.x, 0.5, "x moves left so the box stays inside");
        assert_eq!(crop.h, MIN_CROP_SIZE);
        assert_eq!(crop.y, 0.0);
    }

    #[test]
    fn curves_are_sorted_unique_and_capped() {
        let mut r = Recipe::default();
        r.tone.curve.master = vec![
            [0.8, 0.9],
            [0.2, 2.0],
            [0.2, 0.1],
            [f32::NAN, 0.5],
            [-1.0, 0.0],
        ];
        let master = r.clamped().tone.curve.master;
        assert_eq!(master, vec![[0.0, 0.0], [0.2, 1.0], [0.8, 0.9]]);

        let mut r = Recipe::default();
        r.tone.curve.r = (0..40).map(|i| [i as f32 / 40.0, 0.5]).collect();
        assert_eq!(r.clamped().tone.curve.r.len(), CURVE_POINTS_MAX);
    }

    #[test]
    fn white_stays_above_black() {
        let mut r = Recipe::default();
        r.invert = Invert {
            black: [3.0, 0.5, 9.99],
            white: [1.0, 0.5, 0.0],
        };
        let inv = r.clamped().invert;
        for i in 0..3 {
            assert!(inv.white[i] > inv.black[i]);
        }
    }

    #[test]
    fn dust_and_base_are_limited() {
        let mut r = Recipe::default();
        r.dust.grow_px = 500;
        r.dust.threshold = -1.0;
        r.base.rgb = [0.0, -4.0, f32::NAN];
        let r = r.clamped();
        assert_eq!(r.dust.grow_px, DUST_GROW_PX_MAX);
        assert_eq!(r.dust.threshold, 0.0);
        assert_eq!(r.base.rgb, [BASE_MIN, BASE_MIN, 1.0]);
    }
}
