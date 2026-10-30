//! Film-base normalization: `T = I / base` per channel (docs/specification.md section 4.2 step 4).

use gtp_core::{AppError, ErrorCode, ImageF32};

/// Smallest divisor used for a base channel; a smaller or zero base cannot produce infinity.
pub const EPSILON: f32 = 1e-6;

/// Divides every pixel by the film base, channel by channel, in place, so the base maps to 1.0.
///
/// The result is always finite: a base channel below [`EPSILON`] is raised to it, a NaN sample
/// becomes 0, and an overflowing quotient saturates at `f32::MAX`. Negative samples (noise below
/// black) stay negative. The image must have 3 channels and the base must be finite.
pub fn normalize(image: &mut ImageF32, base: [f32; 3]) -> Result<(), AppError> {
    if image.channels != 3 {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "Film-base normalization needs a 3-channel image.",
        )
        .with_details(format!("{} channels", image.channels)));
    }
    if base.iter().any(|b| !b.is_finite()) {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The film base must be a finite colour.",
        )
        .with_details(format!("{base:?}")));
    }
    let divisor = base.map(|b| b.max(EPSILON));
    let (pixels, _) = image.data.as_chunks_mut::<3>();
    for pixel in pixels {
        for (sample, divisor) in pixel.iter_mut().zip(divisor) {
            *sample = if sample.is_nan() {
                0.0
            } else {
                (*sample / divisor).clamp(-f32::MAX, f32::MAX)
            };
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use gtp_core::testing::synthetic::{self, Params};
    use proptest::array::uniform3;
    use proptest::collection::vec;
    use proptest::num::f32::ANY;
    use proptest::prelude::*;

    use super::*;

    fn image(data: Vec<f32>) -> ImageF32 {
        let pixels = data.len() / 3;
        ImageF32::new(pixels as u32, 1, 3, data).unwrap()
    }

    #[test]
    fn the_base_maps_to_one() {
        let base = [0.8, 0.55, 0.35];
        let mut img = image(vec![0.8, 0.55, 0.35, 0.4, 0.275, 0.175]);
        normalize(&mut img, base).unwrap();
        for (got, want) in img.data.iter().zip([1.0, 1.0, 1.0, 0.5, 0.5, 0.5]) {
            assert!((got - want).abs() < 1e-6, "{got} vs {want}");
        }
    }

    #[test]
    fn zero_input_and_zero_base_never_give_nan_or_infinity() {
        let mut img = image(vec![0.0, 0.0, 0.0, 0.5, 0.5, 0.5]);
        normalize(&mut img, [0.0, -1.0, 0.0]).unwrap();
        assert!(img.data.iter().all(|v| v.is_finite()), "{:?}", img.data);
        assert_eq!(&img.data[..3], [0.0, 0.0, 0.0]);

        let mut img = image(vec![0.0, 0.0, 0.0]);
        normalize(&mut img, [0.5, 0.5, 0.5]).unwrap();
        assert_eq!(img.data, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn non_finite_samples_are_made_finite_and_negative_noise_is_kept() {
        let mut img = image(vec![
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            -0.01,
            3.0e38,
            0.1,
        ]);
        normalize(&mut img, [1e-6, 0.5, 1.0]).unwrap();
        assert!(img.data.iter().all(|v| v.is_finite()));
        assert_eq!(img.data[0], 0.0, "NaN becomes 0");
        assert_eq!(img.data[1], f32::MAX, "infinity saturates");
        assert_eq!(img.data[2], -f32::MAX);
        assert!(img.data[3] < 0.0, "negative noise stays negative");
        assert_eq!(img.data[4], f32::MAX, "an overflowing quotient saturates");
    }

    #[test]
    fn bad_input_is_invalid_input() {
        let mut gray = ImageF32::zeros(2, 2, 1).unwrap();
        assert_eq!(
            normalize(&mut gray, [1.0; 3]).unwrap_err().code,
            ErrorCode::InvalidInput
        );
        let mut rgb = ImageF32::zeros(2, 2, 3).unwrap();
        for base in [[f32::NAN, 1.0, 1.0], [1.0, f32::INFINITY, 1.0]] {
            assert_eq!(
                normalize(&mut rgb, base).unwrap_err().code,
                ErrorCode::InvalidInput
            );
        }
    }

    #[test]
    fn a_synthetic_negative_normalizes_to_its_transmittance() {
        let scan = synthetic::negative(4, &Params::default()).unwrap();
        let truth = &scan.truth;
        let mut img = scan.image.clone();
        normalize(&mut img, truth.base).unwrap();
        for (i, (&got, &p)) in img
            .data
            .iter()
            .zip(&truth.expected_positive.data)
            .enumerate()
        {
            let c = i % 3;
            let inside = {
                let pixel = i / 3;
                let (x, y) = (
                    (pixel % img.width as usize) as u32,
                    (pixel / img.width as usize) as u32,
                );
                x >= truth.frame.x
                    && x < truth.frame.x + truth.frame.w
                    && y >= truth.frame.y
                    && y < truth.frame.y + truth.frame.h
            };
            let want = if inside {
                10f32.powf(-(truth.black[c] + p * (truth.white[c] - truth.black[c])))
            } else {
                1.0
            };
            assert!(
                (got - want).abs() <= 1e-4 * want.max(1e-3),
                "sample {i}: {got} vs {want}"
            );
        }
    }

    proptest! {
        #[test]
        fn the_output_is_always_finite_for_any_input(data in vec(ANY, 0..60), base in uniform3(ANY)) {
            let mut data = data;
            data.truncate(data.len() / 3 * 3);
            prop_assume!(!data.is_empty());
            let mut img = image(data);
            match normalize(&mut img, base) {
                Ok(()) => prop_assert!(img.data.iter().all(|v| v.is_finite())),
                Err(err) => prop_assert!(base.iter().any(|b| !b.is_finite()), "unexpected {err}"),
            }
        }
    }
}
