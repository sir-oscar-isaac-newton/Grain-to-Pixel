//! Output colour transform (docs/specification.md section 4.2 step 9): linear Rec.2020 to an
//! export space, with the ICC profile that describes the result.
//!
//! The conversion is done by Little CMS (`lcms2`) with the relative colorimetric intent, from a
//! linear Rec.2020 source profile to one of:
//! - **sRGB**: Little CMS's built-in sRGB profile (IEC 61966-2-1 primaries and curve, D65).
//! - **Adobe RGB (1998) compatible**: D65, gamma 563/256.
//! - **ProPhoto RGB** (ROMM RGB primaries): D50, gamma 1.8. The D65 to D50 white change is the
//!   chromatic adaptation Little CMS applies through its D50 connection space (Bradford).
//!
//! Colours above white are brought into `[0, 1]` first by [`gtp_core::bring_into_range`], which keeps
//! the hue of a colour with a channel above 1 (ADR 0019) and treats every pixel with no channel
//! above 1 as clipping each channel would (NaN becomes 0). What lies in range but outside the
//! target space is clipped per channel by Little CMS.
//!
//! A black-and-white frame can instead be written as one grey channel ([`to_grey_output`]): a
//! grey profile with the D65 white point and the sRGB tone curve, whatever the output space is
//! (ADR 0018). The grey value is the luminance Y of the linear Rec.2020 pixel, encoded with that
//! curve, which is what the sRGB export of the same pixel has when its three channels are equal.

use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ErrorCode, ImageF32};
use lcms2::{
    CIExyY, CIExyYTRIPLE, DisallowCache, Flags, Intent, Locale, PixelFormat, Profile, Tag,
    TagSignature, ThreadContext, ToneCurve, Transform, MLU,
};
use rayon::prelude::*;

/// Pixels per parallel chunk of the transform.
const CHUNK_PIXELS: usize = 64 * 1024;

const D65: CIExyY = CIExyY {
    x: 0.3127,
    y: 0.3290,
    Y: 1.0,
};
const D50: CIExyY = CIExyY {
    x: 0.3457,
    y: 0.3585,
    Y: 1.0,
};

fn primaries(r: (f64, f64), g: (f64, f64), b: (f64, f64)) -> CIExyYTRIPLE {
    let xy = |(x, y): (f64, f64)| CIExyY { x, y, Y: 1.0 };
    CIExyYTRIPLE {
        Red: xy(r),
        Green: xy(g),
        Blue: xy(b),
    }
}

/// An image converted to an export space: 16-bit RGB, interleaved, encoded with that space's
/// transfer curve, plus the ICC profile to embed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tiff16Output {
    pub width: u32,
    pub height: u32,
    pub space: OutputSpace,
    pub data: Vec<u16>,
    pub icc: Vec<u8>,
}

/// An image converted to one grey channel: 16-bit, encoded with the sRGB tone curve, plus the
/// grey ICC profile to embed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grey16Output {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u16>,
    pub icc: Vec<u8>,
}

fn cms_error(message: &str, err: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal, message).with_details(err.to_string())
}

fn with_description(
    mut profile: Profile<ThreadContext>,
    text: &str,
) -> Result<Profile<ThreadContext>, AppError> {
    let mut description = MLU::new(1);
    description.set_text(text, Locale::none());
    profile.remove_tag(TagSignature::ProfileDescriptionTag);
    if !profile.write_tag(TagSignature::ProfileDescriptionTag, Tag::MLU(&description)) {
        return Err(cms_error("Could not build a colour profile.", text));
    }
    Ok(profile)
}

fn rgb_profile(
    context: &ThreadContext,
    white: &CIExyY,
    primaries: &CIExyYTRIPLE,
    gamma: f64,
    description: &str,
) -> Result<Profile<ThreadContext>, AppError> {
    let curve = ToneCurve::new(gamma);
    let profile = Profile::new_rgb_context(context, white, primaries, &[&curve, &curve, &curve])
        .map_err(|err| cms_error("Could not build a colour profile.", err))?;
    with_description(profile, description)
}

/// The linear Rec.2020 working space as a profile.
fn working_profile(context: &ThreadContext) -> Result<Profile<ThreadContext>, AppError> {
    let rec2020 = primaries((0.708, 0.292), (0.170, 0.797), (0.131, 0.046));
    rgb_profile(context, &D65, &rec2020, 1.0, "Linear Rec.2020")
}

fn output_profile(
    context: &ThreadContext,
    space: OutputSpace,
) -> Result<Profile<ThreadContext>, AppError> {
    match space {
        OutputSpace::Srgb => with_description(Profile::new_srgb_context(context), "sRGB"),
        OutputSpace::AdobeRgb => rgb_profile(
            context,
            &D65,
            &primaries((0.64, 0.33), (0.21, 0.71), (0.15, 0.06)),
            563.0 / 256.0,
            "Adobe RGB (1998) compatible",
        ),
        OutputSpace::Prophoto => rgb_profile(
            context,
            &D50,
            &primaries((0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001)),
            1.8,
            "ProPhoto RGB",
        ),
    }
}

/// The grey profile: D65 white and the sRGB parametric tone curve (type 4: gamma 2.4,
/// `a = 1/1.055`, `b = 0.055/1.055`, `c = 1/12.92`, `d = 0.04045`).
fn grey_profile(context: &ThreadContext) -> Result<Profile<ThreadContext>, AppError> {
    let curve =
        ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
            .map_err(|err| cms_error("Could not build a colour profile.", err))?;
    let profile = Profile::new_gray_context(context, &D65, &curve)
        .map_err(|err| cms_error("Could not build a colour profile.", err))?;
    with_description(profile, "Grey (sRGB tone curve)")
}

/// The ICC profile embedded in greyscale files (the same whatever the output space).
pub fn grey_icc_profile() -> Result<Vec<u8>, AppError> {
    grey_profile(&ThreadContext::new())?
        .icc()
        .map_err(|err| cms_error("Could not write a colour profile.", err))
}

/// The ICC profile embedded in files exported in `space`.
pub fn icc_profile(space: OutputSpace) -> Result<Vec<u8>, AppError> {
    output_profile(&ThreadContext::new(), space)?
        .icc()
        .map_err(|err| cms_error("Could not write a colour profile.", err))
}

/// Converts a linear Rec.2020 image (3 channels) to `space`.
pub fn to_output(image: &ImageF32, space: OutputSpace) -> Result<Tiff16Output, AppError> {
    if image.channels != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Only a 3-channel image can be exported.",
        )
        .with_details(format!("{} channels", image.channels)));
    }
    let context = ThreadContext::new();
    let source = working_profile(&context)?;
    let target = output_profile(&context, space)?;
    let icc = target
        .icc()
        .map_err(|err| cms_error("Could not write a colour profile.", err))?;
    // Without the cache the transform may be shared by the worker threads.
    let transform: Transform<[f32; 3], [u16; 3], ThreadContext, DisallowCache> =
        Transform::new_flags_context(
            &context,
            &source,
            PixelFormat::RGB_FLT,
            &target,
            PixelFormat::RGB_16,
            Intent::RelativeColorimetric,
            Flags::NO_CACHE,
        )
        .map_err(|err| cms_error("Could not build the colour transform.", err))?;

    let (pixels, _) = image.data.as_chunks::<3>();
    let mut out = vec![[0u16; 3]; pixels.len()];
    out.par_chunks_mut(CHUNK_PIXELS)
        .zip(pixels.par_chunks(CHUNK_PIXELS))
        .for_each(|(dst, src)| {
            let clean: Vec<[f32; 3]> = src.iter().map(|p| gtp_core::bring_into_range(*p)).collect();
            transform.transform_pixels(&clean, dst);
        });

    Ok(Tiff16Output {
        width: image.width,
        height: image.height,
        space,
        data: out.into_flattened(),
        icc,
    })
}

/// Converts a linear Rec.2020 image (3 channels) to one grey channel with the sRGB tone curve.
pub fn to_grey_output(image: &ImageF32) -> Result<Grey16Output, AppError> {
    if image.channels != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Only a 3-channel image can be exported.",
        )
        .with_details(format!("{} channels", image.channels)));
    }
    let context = ThreadContext::new();
    let source = working_profile(&context)?;
    let target = grey_profile(&context)?;
    let icc = target
        .icc()
        .map_err(|err| cms_error("Could not write a colour profile.", err))?;
    let transform: Transform<[f32; 3], u16, ThreadContext, DisallowCache> =
        Transform::new_flags_context(
            &context,
            &source,
            PixelFormat::RGB_FLT,
            &target,
            PixelFormat::GRAY_16,
            Intent::RelativeColorimetric,
            Flags::NO_CACHE,
        )
        .map_err(|err| cms_error("Could not build the colour transform.", err))?;

    let (pixels, _) = image.data.as_chunks::<3>();
    let mut out = vec![0u16; pixels.len()];
    out.par_chunks_mut(CHUNK_PIXELS)
        .zip(pixels.par_chunks(CHUNK_PIXELS))
        .for_each(|(dst, src)| {
            let clean: Vec<[f32; 3]> = src.iter().map(|p| gtp_core::bring_into_range(*p)).collect();
            transform.transform_pixels(&clean, dst);
        });

    Ok(Grey16Output {
        width: image.width,
        height: image.height,
        data: out,
        icc,
    })
}

#[cfg(test)]
mod tests {
    use lcms2::ColorSpaceSignature;

    use super::*;

    fn convert(rgb: [f32; 3], space: OutputSpace) -> [u16; 3] {
        let img = ImageF32::new(1, 1, 3, rgb.to_vec()).unwrap();
        let out = to_output(&img, space).unwrap();
        [out.data[0], out.data[1], out.data[2]]
    }

    fn close(got: [u16; 3], want: [u16; 3], tolerance: u16) -> bool {
        got.iter()
            .zip(want)
            .all(|(g, w)| g.abs_diff(w) <= tolerance)
    }

    const ALL: [OutputSpace; 3] = [
        OutputSpace::Srgb,
        OutputSpace::AdobeRgb,
        OutputSpace::Prophoto,
    ];

    #[test]
    fn white_grey_and_black_stay_neutral_in_every_space() {
        for space in ALL {
            assert!(
                close(convert([1.0; 3], space), [65535; 3], 2),
                "{space:?} white"
            );
            assert_eq!(convert([0.0; 3], space), [0; 3], "{space:?} black");
            let grey = convert([0.18; 3], space);
            assert!(
                grey[0].abs_diff(grey[1]) <= 2 && grey[1].abs_diff(grey[2]) <= 2,
                "{space:?} grey {grey:?}"
            );
        }
    }

    #[test]
    fn mid_grey_follows_each_transfer_curve() {
        // sRGB: 0.18 linear is 0.4614 encoded; Adobe RGB 0.18^(256/563); ProPhoto 0.18^(1/1.8).
        let expected = [
            (OutputSpace::Srgb, 0.461_356),
            (OutputSpace::AdobeRgb, 0.18f64.powf(256.0 / 563.0)),
            (OutputSpace::Prophoto, 0.18f64.powf(1.0 / 1.8)),
        ];
        for (space, want) in expected {
            let got = f64::from(convert([0.18; 3], space)[1]) / 65535.0;
            assert!((got - want).abs() < 0.002, "{space:?}: {got} vs {want}");
        }
    }

    #[test]
    fn the_srgb_primaries_come_out_pure() {
        // sRGB red, green and blue expressed in linear Rec.2020 (ITU-R BT.2087 matrix).
        let cases = [
            ([0.627_404, 0.069_097, 0.016_391], [65535, 0, 0]),
            ([0.329_283, 0.919_540, 0.088_013], [0, 65535, 0]),
            ([0.043_313, 0.011_362, 0.895_595], [0, 0, 65535]),
        ];
        for (rec2020, want) in cases {
            let got = convert(rec2020, OutputSpace::Srgb);
            assert!(close(got, want, 200), "{rec2020:?} -> {got:?}");
        }
    }

    #[test]
    fn out_of_range_and_nan_values_are_clipped() {
        let img = ImageF32::new(2, 1, 3, vec![5.0, -1.0, f32::NAN, 1.0, 1.0, 1.0]).unwrap();
        let out = to_output(&img, OutputSpace::Srgb).unwrap();
        assert_eq!(out.data.len(), 6);
        // Red 5 with nothing else keeps its hue and rolls off towards white (ADR 0019): red is
        // the largest channel and green and blue are no longer 0.
        assert!(out.data[1] > 0);
        assert!(out.data[0] > out.data[1]);
    }

    /// Clipping each channel on its own, the rule before P-32, with a transform built here.
    fn old_clamp_output(
        image: &ImageF32,
        target: &Profile<ThreadContext>,
        context: &ThreadContext,
    ) -> Vec<[u16; 3]> {
        let source = working_profile(context).unwrap();
        let transform: Transform<[f32; 3], [u16; 3], ThreadContext, DisallowCache> =
            Transform::new_flags_context(
                context,
                &source,
                PixelFormat::RGB_FLT,
                target,
                PixelFormat::RGB_16,
                Intent::RelativeColorimetric,
                Flags::NO_CACHE,
            )
            .unwrap();
        let clean: Vec<[f32; 3]> = image
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p.map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }))
            .collect();
        let mut out = vec![[0u16; 3]; clean.len()];
        transform.transform_pixels(&clean, &mut out);
        out
    }

    /// Pixels with no channel above 1: a grid, plus NaN, negative, zero and just-below-one values.
    fn in_range_image() -> ImageF32 {
        let levels = [
            f32::NAN,
            -2.0,
            -0.0,
            0.0,
            1.0e-4,
            0.003,
            0.1,
            0.18,
            0.4,
            0.75,
            0.999_999,
            1.0,
        ];
        let mut data = Vec::new();
        for r in levels {
            for g in levels {
                for b in levels {
                    data.extend([r, g, b]);
                }
            }
        }
        let pixels = data.len() as u32 / 3;
        ImageF32::new(pixels, 1, 3, data).unwrap()
    }

    #[test]
    fn pixels_with_no_channel_above_one_give_the_bytes_of_per_channel_clipping() {
        let image = in_range_image();
        for space in ALL {
            let context = ThreadContext::new();
            let target = output_profile(&context, space).unwrap();
            let want = old_clamp_output(&image, &target, &context);
            let got = to_output(&image, space).unwrap();
            assert_eq!(got.data, want.into_flattened(), "{space:?}");
        }
    }

    #[test]
    fn the_grey_output_of_in_range_pixels_is_the_bytes_of_per_channel_clipping() {
        let image = in_range_image();
        let context = ThreadContext::new();
        let source = working_profile(&context).unwrap();
        let target = grey_profile(&context).unwrap();
        let transform: Transform<[f32; 3], u16, ThreadContext, DisallowCache> =
            Transform::new_flags_context(
                &context,
                &source,
                PixelFormat::RGB_FLT,
                &target,
                PixelFormat::GRAY_16,
                Intent::RelativeColorimetric,
                Flags::NO_CACHE,
            )
            .unwrap();
        let clean: Vec<[f32; 3]> = image
            .data
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p.map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }))
            .collect();
        let mut want = vec![0u16; clean.len()];
        transform.transform_pixels(&clean, &mut want);
        assert_eq!(to_grey_output(&image).unwrap().data, want);
    }

    /// `to_output` to sRGB as it was before P-32 (each channel clipped on its own), for timing.
    fn old_to_output_srgb(image: &ImageF32) -> Vec<u16> {
        let context = ThreadContext::new();
        let source = working_profile(&context).unwrap();
        let target = output_profile(&context, OutputSpace::Srgb).unwrap();
        let transform: Transform<[f32; 3], [u16; 3], ThreadContext, DisallowCache> =
            Transform::new_flags_context(
                &context,
                &source,
                PixelFormat::RGB_FLT,
                &target,
                PixelFormat::RGB_16,
                Intent::RelativeColorimetric,
                Flags::NO_CACHE,
            )
            .unwrap();
        let (pixels, _) = image.data.as_chunks::<3>();
        let mut out = vec![[0u16; 3]; pixels.len()];
        out.par_chunks_mut(CHUNK_PIXELS)
            .zip(pixels.par_chunks(CHUNK_PIXELS))
            .for_each(|(dst, src)| {
                let clean: Vec<[f32; 3]> = src
                    .iter()
                    .map(|p| p.map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }))
                    .collect();
                transform.transform_pixels(&clean, dst);
            });
        out.into_flattened()
    }

    /// Timing for ADR 0019: median of seven runs of the sRGB export of 24 megapixels, with the new
    /// rule and with the old clip. Run with
    /// `cargo test --release -p gtp-io measure_time_export -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing run, not a check"]
    fn measure_time_export() {
        use std::time::Instant;
        let median = |f: &dyn Fn()| {
            f();
            let mut runs: Vec<_> = (0..7)
                .map(|_| {
                    let started = Instant::now();
                    f();
                    started.elapsed()
                })
                .collect();
            runs.sort();
            runs[3]
        };
        eprintln!(
            "rayon threads in the global pool: {}",
            rayon::current_num_threads()
        );
        let (w, h) = (6000usize, 4000usize);
        for (label, share) in [
            ("no pixel above 1", 0.0f32),
            ("half the pixels above 1", 0.5),
        ] {
            let mut state = 0x9E37_79B9u32;
            let data: Vec<f32> = (0..w * h * 3)
                .map(|i| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    let x = (i / 3 % w) as f32 / w as f32;
                    let base = x * 0.9 + (state % 1000) as f32 / 10_000.0;
                    if share > 0.0 && x > 1.0 - share && i % 3 != 2 {
                        base * 3.0
                    } else {
                        base
                    }
                })
                .collect();
            let image = ImageF32::new(w as u32, h as u32, 3, data).unwrap();
            let new = median(&|| {
                to_output(&image, OutputSpace::Srgb).unwrap();
            });
            let old = median(&|| {
                old_to_output_srgb(&image);
            });
            eprintln!("6000x4000 to_output (sRGB), {label}: new rule {new:?}, old clip {old:?}");
        }
    }

    #[test]
    fn a_bright_saturated_colour_keeps_its_hue_in_every_space() {
        // ADR 0015's green: per-channel clipping gave (0.6, 1, 0.9), a pale cyan-white.
        for space in ALL {
            let [r, g, b] = convert([0.6, 2.7, 0.9], space);
            assert!(g > b && b > r, "{space:?}: {r} {g} {b}");
        }
    }

    #[test]
    fn the_profiles_parse_back_as_rgb_with_their_names() {
        for (space, name) in [
            (OutputSpace::Srgb, "sRGB"),
            (OutputSpace::AdobeRgb, "Adobe RGB (1998) compatible"),
            (OutputSpace::Prophoto, "ProPhoto RGB"),
        ] {
            let icc = icc_profile(space).unwrap();
            let parsed = Profile::new_icc(&icc).unwrap();
            assert_eq!(parsed.color_space(), ColorSpaceSignature::RgbData);
            let description = parsed
                .info(lcms2::InfoType::Description, Locale::none())
                .unwrap_or_default();
            assert_eq!(description, name);
        }
    }

    #[test]
    fn the_grey_profile_parses_back_as_grey_with_its_name() {
        let icc = grey_icc_profile().unwrap();
        let parsed = Profile::new_icc(&icc).unwrap();
        assert_eq!(parsed.color_space(), ColorSpaceSignature::GrayData);
        let description = parsed
            .info(lcms2::InfoType::Description, Locale::none())
            .unwrap_or_default();
        assert_eq!(description, "Grey (sRGB tone curve)");
    }

    #[test]
    fn grey_follows_the_srgb_curve_and_keeps_black_and_white() {
        let grey = |v: f32| {
            let img = ImageF32::new(1, 1, 3, vec![v; 3]).unwrap();
            to_grey_output(&img).unwrap().data[0]
        };
        assert_eq!(grey(0.0), 0);
        assert!(65535 - grey(1.0) <= 2);
        let got = f64::from(grey(0.18)) / 65535.0;
        assert!((got - 0.461_356).abs() < 0.002, "{got}");
        let out = to_grey_output(
            &ImageF32::new(2, 1, 3, vec![5.0, -1.0, f32::NAN, 1.0, 1.0, 1.0]).unwrap(),
        )
        .unwrap();
        assert_eq!(out.data.len(), 2);
    }

    #[test]
    fn a_non_rgb_image_is_rejected_for_grey_too() {
        let gray = ImageF32::zeros(2, 2, 1).unwrap();
        assert_eq!(
            to_grey_output(&gray).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn a_non_rgb_image_is_rejected() {
        let gray = ImageF32::zeros(2, 2, 1).unwrap();
        let err = to_output(&gray, OutputSpace::Srgb).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidInput);
    }
}
