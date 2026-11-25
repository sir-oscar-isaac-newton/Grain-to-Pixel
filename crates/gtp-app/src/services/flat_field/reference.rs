//! The flat-field build as it was before the film carrier was left out (P-21, decision 0010): the
//! mean of every sample of every cell. Kept for the tests: a capture with no carrier in it must give
//! exactly this (decisions/0021-flat-field-ignores-the-carrier.md), and the measurements compare it
//! with the new method.

use gtp_core::{AppError, ExposureInfo, FlatField, ImageF32};

use super::{invalid, GRID_WIDTH, SAMPLE_STEP};

/// The plain method: every sample counts, whatever it is.
pub(super) fn build_flat_field_reference(
    image: &ImageF32,
    sources: Vec<String>,
) -> Result<FlatField, AppError> {
    if image.channels != 3 {
        return Err(invalid(
            "A flat field needs a picture of three colours.",
            format!("{} channels", image.channels),
        ));
    }
    let (w, h) = (image.width as usize, image.height as usize);
    let grid_w = (GRID_WIDTH as usize).min(w);
    let grid_h = ((h * grid_w).div_ceil(w)).clamp(2, h);
    if grid_w < 2 || h < 2 {
        return Err(invalid(
            "The picture is too small to make a flat field from.",
            format!("{w}x{h}"),
        ));
    }

    let cell = |ch: usize, gx: usize, gy: usize| -> f64 {
        let (x0, x1) = (
            gx * w / grid_w,
            ((gx + 1) * w / grid_w).max(gx * w / grid_w + 1),
        );
        let (y0, y1) = (
            gy * h / grid_h,
            ((gy + 1) * h / grid_h).max(gy * h / grid_h + 1),
        );
        let (mut sum, mut n) = (0.0f64, 0u64);
        for y in (y0..y1).step_by(SAMPLE_STEP) {
            for x in (x0..x1).step_by(SAMPLE_STEP) {
                let v = image.data[(y * w + x) * 3 + ch];
                if v.is_finite() {
                    sum += f64::from(v);
                    n += 1;
                }
            }
        }
        sum / n.max(1) as f64
    };

    let mut light = vec![0.0f32; grid_w * grid_h * 3];
    for ch in 0..3 {
        let means: Vec<f64> = (0..grid_h)
            .flat_map(|gy| (0..grid_w).map(move |gx| (gx, gy)))
            .map(|(gx, gy)| cell(ch, gx, gy))
            .collect();
        let (cx, cy) = (grid_w / 2, grid_h / 2);
        let centre: f64 = (cy.saturating_sub(1)..=(cy + 1).min(grid_h - 1))
            .flat_map(|gy| {
                (cx.saturating_sub(1)..=(cx + 1).min(grid_w - 1)).map(move |gx| (gx, gy))
            })
            .map(|(gx, gy)| means[gy * grid_w + gx])
            .sum::<f64>()
            / 9.0;
        if centre <= 1e-6 || !centre.is_finite() {
            return Err(invalid(
                "The bare-light capture is too dark to use.",
                format!("channel {ch}: centre {centre}"),
            ));
        }
        for (i, mean) in means.iter().enumerate() {
            light[i * 3 + ch] = ((*mean / centre) as f32).max(FlatField::MIN_LIGHT);
        }
    }
    let flat = FlatField {
        grid_w: grid_w as u32,
        grid_h: grid_h as u32,
        frame_w: image.width,
        frame_h: image.height,
        light,
        sources,
        exposure: ExposureInfo::default(),
        files: Vec::new(),
        created_at: None,
    };
    if flat.is_valid() {
        Ok(flat)
    } else {
        Err(AppError::internal("the flat field built is not valid"))
    }
}
