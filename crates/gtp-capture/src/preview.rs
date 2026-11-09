//! The camera's live view (H-14): one JPEG frame at a time, so the capture view can frame and focus
//! the film (the lens is on manual) and judge the exposure before a triplet is taken. The libgphoto2
//! calls are in `gphoto/session.rs`; the worker that leaves live view before every capture is in
//! `gphoto/worker.rs`. H-18 adds the camera's own zoomed live view (its zoom steps, and moving the
//! zoomed area) and the reason live view cannot start; the plain types and checks for those are
//! here and build and are tested on every platform.

use gtp_core::{AppError, ErrorCode};
use serde::Serialize;

use crate::settings::{clean_text, clean_value};

/// What the whole-frame (unzoomed) step is called in the app.
pub const WHOLE_FRAME_LABEL: &str = "Whole frame";
/// The sensor size (width, height in pixels) assumed for the zoomed area when the camera does not
/// report its image size; it is the D600's.
pub const FALLBACK_SENSOR_PX: (u32, u32) = (6016, 4016);
/// The most characters kept of a live-view prohibit condition.
const MAX_PROHIBIT_CHARS: usize = 120;
/// The words libgphoto2 puts before the conditions in its `liveviewprohibit` text.
const PROHIBIT_PREFIX: &str = "live view prohibit conditions:";

/// One zoom step of the camera's live view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ZoomStep {
    /// The camera's own choice (`"100%"`, `"Entire Display"`), which is what is set.
    pub value: String,
    /// The words for the user: `"Whole frame"` for the unzoomed view, otherwise the choice.
    pub label: String,
}

/// True when `value` is the camera's unzoomed choice ("Entire Display" on Nikon bodies).
#[must_use]
pub fn is_whole_frame(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("entire")
}

/// The zoom steps in the choices the camera lists: cleaned like every device text, without the
/// choices libgphoto2 shows as "Unknown value ..." (the D600 lists 1, 3 and 5 that way).
#[must_use]
pub fn zoom_steps_from_choices(choices: impl IntoIterator<Item = String>) -> Vec<ZoomStep> {
    choices
        .into_iter()
        .filter_map(|choice| clean_text(&choice))
        .filter(|choice| !choice.to_ascii_lowercase().starts_with("unknown value"))
        .map(|value| ZoomStep {
            label: if is_whole_frame(&value) {
                WHOLE_FRAME_LABEL.to_owned()
            } else {
                value.clone()
            },
            value,
        })
        .collect()
}

/// Checks that `value` is one of the camera's current zoom steps and returns it cleaned.
pub fn check_zoom(steps: &[ZoomStep], value: &str) -> Result<String, AppError> {
    if steps.is_empty() {
        return Err(AppError::new(
            ErrorCode::InvalidInput,
            "The camera has no live-view zoom.",
        ));
    }
    let value = clean_value(value).map_err(|_| zoom_not_offered())?;
    if steps.iter().any(|step| step.value == value) {
        Ok(value)
    } else {
        Err(zoom_not_offered())
    }
}

fn zoom_not_offered() -> AppError {
    AppError::new(
        ErrorCode::InvalidInput,
        "That zoom step is not one the camera offers.",
    )
}

/// Checks that a point on the frame is given as two fractions between 0 and 1.
pub fn check_area(x: f32, y: f32) -> Result<(), AppError> {
    let ok = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
    if ok(x) && ok(y) {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::InvalidInput,
            "The zoomed area must be a point on the frame, between 0 and 1 in both directions.",
        ))
    }
}

/// The sensor size in the camera's `imagesize` text (`"6016x4016"`), or `None` when it is not two
/// positive numbers.
#[must_use]
pub fn parse_sensor_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.trim().split_once(['x', 'X'])?;
    let (w, h) = (w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// The text for the camera's `changeafarea`: the point (`x`, `y` as fractions of the frame) in
/// sensor pixels, `"{X}x{Y}"`, rounded and kept inside the sensor. The sensor size comes from the
/// camera's `imagesize` text when it has one, else [`FALLBACK_SENSOR_PX`]. The fractions are
/// clamped to 0..=1 (callers check them with [`check_area`] first).
#[must_use]
pub fn area_text(x: f32, y: f32, image_size: Option<&str>) -> String {
    let (w, h) = image_size
        .and_then(parse_sensor_size)
        .unwrap_or(FALLBACK_SENSOR_PX);
    let px = |fraction: f32, size: u32| -> u32 {
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Both are small positive numbers, so the casts cannot lose anything that matters.
        let value = (f64::from(fraction) * f64::from(size)).round() as u32;
        value.min(size - 1)
    };
    format!("{}x{}", px(x, w), px(y, h))
}

/// The condition in a `liveviewprohibit` text (`"Live View prohibit conditions: Sequence error"`),
/// or `None` when the text names none.
#[must_use]
pub fn prohibit_reason(text: &str) -> Option<String> {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    let rest = if clean.to_ascii_lowercase().starts_with(PROHIBIT_PREFIX) {
        &clean[PROHIBIT_PREFIX.len()..]
    } else {
        clean
    };
    let rest = rest.trim().trim_end_matches('.').trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest.chars().take(MAX_PROHIBIT_CHARS).collect())
    }
}

/// Reads live-view frames from a camera. Kept apart from `CameraSource` for the reason
/// `CameraSettings` is: a job holds the source for its whole run, while the UI asks for frames
/// between jobs. The live view never touches the light. Callers refuse frames while a capture runs;
/// a backend also refuses while a picture is being taken, and leaves live view itself before taking
/// one.
pub trait CameraPreview: Send + Sync {
    /// One live-view frame as JPEG bytes. The first call puts the camera in live view.
    fn preview_frame(&self) -> Result<Vec<u8>, AppError>;
    /// Leaves live view. Does nothing when the camera is not in live view.
    fn preview_stop(&self) -> Result<(), AppError>;
    /// The zoom steps the camera offers, whole frame first as the camera lists them; empty when
    /// the camera has no live-view zoom.
    fn preview_zoom_steps(&self) -> Result<Vec<ZoomStep>, AppError>;
    /// Sets the zoom to one of [`CameraPreview::preview_zoom_steps`]'s values (refused otherwise).
    /// Puts the camera in live view if it is not in it. The zoom is reset to the whole frame when
    /// live view is left.
    fn set_preview_zoom(&self, value: &str) -> Result<(), AppError>;
    /// Moves the zoomed area to the point (`x`, `y`) of the frame, as fractions from 0 to 1.
    /// Puts the camera in live view if it is not in it.
    fn set_preview_area(&self, x: f32, y: f32) -> Result<(), AppError>;
    /// The condition that stops live view from starting now ("Sequence error"), if the camera
    /// names one.
    fn preview_prohibit(&self) -> Result<Option<String>, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(list: &[&str]) -> Vec<String> {
        list.iter().map(|c| (*c).to_owned()).collect()
    }

    #[test]
    fn steps_leave_out_unknown_values_and_word_the_whole_frame() {
        let steps = zoom_steps_from_choices(choices(&[
            "Entire Display",
            "Unknown value 0001",
            "25%",
            "unknown value 0003",
            "50%",
            "",
            "100%",
            "200%",
        ]));
        let pairs: Vec<(&str, &str)> = steps
            .iter()
            .map(|s| (s.value.as_str(), s.label.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("Entire Display", "Whole frame"),
                ("25%", "25%"),
                ("50%", "50%"),
                ("100%", "100%"),
                ("200%", "200%"),
            ]
        );
        assert!(zoom_steps_from_choices(Vec::new()).is_empty());
    }

    #[test]
    fn a_zoom_must_be_one_of_the_steps() {
        let steps = zoom_steps_from_choices(choices(&["Entire Display", "100%"]));
        assert_eq!(check_zoom(&steps, " 100% ").unwrap(), "100%");
        for bad in ["300%", "", "Unknown value 0001"] {
            let err = check_zoom(&steps, bad).unwrap_err();
            assert_eq!(err.code, ErrorCode::InvalidInput);
            assert!(err.message.contains("not one the camera offers"));
        }
        let none = check_zoom(&[], "100%").unwrap_err();
        assert!(none.message.contains("no live-view zoom"));
    }

    #[test]
    fn the_area_must_be_a_point_on_the_frame() {
        for (x, y) in [(0.0, 0.0), (1.0, 1.0), (0.3, 0.7)] {
            assert!(check_area(x, y).is_ok());
        }
        for (x, y) in [
            (-0.01, 0.5),
            (0.5, 1.01),
            (f32::NAN, 0.5),
            (0.5, f32::INFINITY),
        ] {
            assert_eq!(check_area(x, y).unwrap_err().code, ErrorCode::InvalidInput);
        }
    }

    #[test]
    fn the_area_is_in_sensor_pixels_from_the_image_size_or_the_d600_fallback() {
        assert_eq!(area_text(0.5, 0.5, Some("6016x4016")), "3008x2008");
        assert_eq!(area_text(0.5, 0.5, None), "3008x2008");
        assert_eq!(area_text(0.0, 0.0, None), "0x0");
        assert_eq!(area_text(1.0, 1.0, None), "6015x4015", "kept inside");
        assert_eq!(area_text(0.25, 0.75, Some("4000x3000")), "1000x2250");
        assert_eq!(area_text(0.3, 0.3, Some("4000 x 3000")), "1200x900");
        assert_eq!(area_text(0.3, 0.7, Some("Large")), "1805x2811", "fallback");
        assert_eq!(area_text(0.5, 0.5, Some("0x0")), "3008x2008");
        assert_eq!(parse_sensor_size("6016X4016"), Some((6016, 4016)));
        assert_eq!(parse_sensor_size("6016"), None);
    }

    #[test]
    fn the_prohibit_condition_is_found_after_the_prefix() {
        assert_eq!(
            prohibit_reason("Live View prohibit conditions: Sequence error").as_deref(),
            Some("Sequence error")
        );
        assert_eq!(
            prohibit_reason("Live View prohibit conditions: Sequence error, Recording.\n")
                .as_deref(),
            Some("Sequence error, Recording")
        );
        assert_eq!(
            prohibit_reason("Sequence error").as_deref(),
            Some("Sequence error")
        );
        assert_eq!(prohibit_reason("Live View prohibit conditions: "), None);
        assert_eq!(prohibit_reason("  "), None);
        assert_eq!(
            prohibit_reason(&"x".repeat(500)).map(|r| r.len()),
            Some(120)
        );
    }
}
