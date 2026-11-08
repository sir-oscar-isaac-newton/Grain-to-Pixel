//! Automatic Scanlight level calibration (H-17): finds, per colour, the level at which the
//! brightest pixels of that colour sit just below clipping.
//!
//! The loop is pure: a [`ChannelMeter`] lights one colour at a level, takes a picture and says how
//! bright the brightest pixels (99.9th percentile of the colour's linear value, 0 to 1) are. The
//! light is linear in its level, so each step is proportional: `level * TARGET / reading`. A clipped
//! reading carries no information about how far over it is, so the level is halved instead (or, once
//! a level below it is known to be fine, bisected). Nothing here saves anything.

use gtp_core::{AppError, CalibrationResult, CalibrationWarning, ErrorCode, TripletChannel};
use serde::{Deserialize, Serialize};

/// What the brightest pixels should read: the middle of [`ACCEPT`].
pub const TARGET: f32 = 0.93;
/// A reading in this range ends a colour's calibration.
pub const ACCEPT: std::ops::RangeInclusive<f32> = 0.90..=0.97;
/// A reading at or above this counts as clipped (the same level the test shot and import use).
pub const CLIP_LEVEL: f32 = 0.99;
/// The shots one colour may take before the calibration gives up on it.
pub const MAX_SHOTS_PER_CHANNEL: u8 = 6;
/// A final level below this warns: one step of it is a large share of the brightness.
pub const COARSE_LEVEL: u8 = 20;
/// The share of the brightest pixels that is ignored (dust, a hot pixel), as a fraction.
pub const PERCENTILE: f32 = 0.999;

/// Takes the pictures of a calibration.
pub trait ChannelMeter {
    /// Lights `channel` alone at `level` (1 to 255), takes a picture and returns the 99.9th
    /// percentile of that channel's linear value (0 to 1) over the measured area. `shot` counts
    /// from 1 within the colour. The light is off again when this returns.
    fn measure(&mut self, channel: TripletChannel, level: u8, shot: u8) -> Result<f32, AppError>;

    /// True once the calibration was asked to stop; checked before every shot.
    fn cancelled(&self) -> bool {
        false
    }
}

/// A rectangle of the frame in fractions of its width and height (0 to 1): the area that
/// calibration measures. `None` where it is optional means the whole frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Region {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub w: f32,
    /// The height.
    pub h: f32,
}

impl Region {
    /// This region if it is finite, has a positive size and lies inside the frame.
    pub fn validated(self) -> Result<Self, AppError> {
        let finite = [self.x, self.y, self.w, self.h]
            .iter()
            .all(|v| v.is_finite());
        if finite
            && self.x >= 0.0
            && self.y >= 0.0
            && self.w > 0.0
            && self.h > 0.0
            && self.x + self.w <= 1.0
            && self.y + self.h <= 1.0
        {
            Ok(self)
        } else {
            Err(AppError::new(
                ErrorCode::InvalidInput,
                "The area to calibrate on must lie inside the frame and have a size.",
            ))
        }
    }

    /// The pixels of a `width` x `height` picture this region covers, as `(x0, y0, x1, y1)` with
    /// the end exclusive; at least one pixel each way.
    pub fn pixels(&self, width: u32, height: u32) -> (usize, usize, usize, usize) {
        let edge = |fraction: f32, size: u32| (fraction * size as f32).round() as usize;
        let (w, h) = (width as usize, height as usize);
        let x0 = edge(self.x, width).min(w.saturating_sub(1));
        let y0 = edge(self.y, height).min(h.saturating_sub(1));
        let x1 = edge(self.x + self.w, width).clamp(x0 + 1, w.max(1));
        let y1 = edge(self.y + self.h, height).clamp(y0 + 1, h.max(1));
        (x0, y0, x1, y1)
    }
}

/// The value below which `fraction` of `values` lie (0 to 1); non-finite values count as 0 and the
/// rest are clamped to 0 to 1. `None` for no values. Reorders `values`.
pub fn percentile(values: &mut [f32], fraction: f32) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    for value in values.iter_mut() {
        *value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
    let rank = (fraction.clamp(0.0, 1.0) * values.len() as f32).ceil() as usize;
    let index = rank.clamp(1, values.len()) - 1;
    let (_, nth, _) = values.select_nth_unstable_by(index, f32::total_cmp);
    Some(*nth)
}

fn colour(channel: TripletChannel) -> &'static str {
    ["Red", "Green", "Blue"][channel.index()]
}

fn fail(message: String) -> AppError {
    AppError::new(ErrorCode::Io, message)
}

fn cancelled() -> AppError {
    AppError::new(ErrorCode::Cancelled, "The calibration was cancelled.")
}

/// Calibrates red, green and blue in turn, each starting from its level of `start` (clamped to
/// 1 to 255). Asks `meter` whether to stop before every shot.
pub fn calibrate(
    meter: &mut dyn ChannelMeter,
    start: [u8; 3],
) -> Result<CalibrationResult, AppError> {
    let mut result = CalibrationResult {
        levels: [0; 3],
        reached: [0.0; 3],
        shots: [0; 3],
        warnings: Vec::new(),
    };
    for channel in TripletChannel::ALL {
        let i = channel.index();
        let (level, reached, shots) = calibrate_channel(meter, channel, start[i].max(1))?;
        result.levels[i] = level;
        result.reached[i] = reached;
        result.shots[i] = shots;
        if level < COARSE_LEVEL {
            result
                .warnings
                .push(CalibrationWarning::CoarseLevel { channel, level });
        }
    }
    Ok(result)
}

/// One colour: returns its final level, the reading there and the shots taken.
fn calibrate_channel(
    meter: &mut dyn ChannelMeter,
    channel: TripletChannel,
    start: u8,
) -> Result<(u8, f32, u8), AppError> {
    let name = colour(channel);
    let mut level = start;
    // The highest level seen unclipped, with its reading, and the lowest level seen clipped: the
    // answer lies between them, so a step never goes outside.
    let mut below: Option<(u8, f32)> = None;
    let mut above: Option<u8> = None;
    for shot in 1..=MAX_SHOTS_PER_CHANNEL {
        if meter.cancelled() {
            return Err(cancelled());
        }
        let reading = meter.measure(channel, level, shot)?;
        let finite = reading.is_finite();
        if finite && ACCEPT.contains(&reading) {
            return Ok((level, reading, shot));
        }
        let next = if finite && reading >= CLIP_LEVEL {
            if level == 1 {
                return Err(fail(format!(
                    "{name} is clipped even at level 1: the exposure is too long. Use a shorter shutter speed and calibrate again."
                )));
            }
            above = Some(above.map_or(level, |a| a.min(level)));
            if below.is_some_and(|(l, _)| l >= level) {
                below = None;
            }
            match below {
                // Nothing but one step apart: the lower level is as close as the light can get.
                Some((l, p)) if level - l <= 1 => return Ok((l, p, shot)),
                Some((l, _)) => ((u16::from(l) + u16::from(level)) / 2) as u8,
                None => (level / 2).max(1),
            }
        } else {
            let dark = !finite || reading < *ACCEPT.start();
            if dark && level == 255 {
                return Err(fail(format!(
                    "{name} is too dark even at level 255: the exposure is too short. Use a longer shutter speed and calibrate again."
                )));
            }
            if below.is_none_or(|(l, _)| level > l) {
                below = Some((level, if finite { reading } else { 0.0 }));
            }
            if above.is_some_and(|a| a <= level) {
                above = None;
            }
            let wanted = if finite && reading > 0.0 {
                (f32::from(level) * TARGET / reading)
                    .round()
                    .clamp(1.0, 255.0) as u8
            } else {
                255
            };
            above.map_or(wanted, |a| wanted.min(a - 1))
        };
        if next == level {
            // One step of the light cannot move this closer.
            return Ok((level, if finite { reading } else { 0.0 }, shot));
        }
        level = next;
    }
    Err(fail(format!(
        "{name} did not settle in {MAX_SHOTS_PER_CHANNEL} shots. Check that the film base fills the frame and nothing moved, then calibrate again."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A linear light: the reading is `k * level`, 1.0 (clipped) from the top.
    struct Linear {
        k: [f32; 3],
        shots: Vec<(TripletChannel, u8)>,
        cancel_after: Option<usize>,
    }

    impl Linear {
        fn new(k: [f32; 3]) -> Self {
            Self {
                k,
                shots: Vec::new(),
                cancel_after: None,
            }
        }
    }

    impl ChannelMeter for Linear {
        fn measure(&mut self, channel: TripletChannel, level: u8, _: u8) -> Result<f32, AppError> {
            self.shots.push((channel, level));
            Ok((self.k[channel.index()] * f32::from(level)).min(1.0))
        }

        fn cancelled(&self) -> bool {
            self.cancel_after.is_some_and(|n| self.shots.len() >= n)
        }
    }

    #[test]
    fn it_converges_from_a_start_that_is_too_high_or_too_low() {
        for start in [255, 200, 100, 44, 10, 1] {
            let mut meter = Linear::new([0.004, 0.008, 0.012]);
            let result = calibrate(&mut meter, [start; 3]).unwrap();
            for i in 0..3 {
                assert!(ACCEPT.contains(&result.reached[i]), "{start}: {result:?}");
                assert!((1..=MAX_SHOTS_PER_CHANNEL).contains(&result.shots[i]));
            }
            for (level, ideal) in result.levels.iter().zip([232.5, 116.25, 77.5]) {
                assert!(
                    (f32::from(*level) - ideal).abs() <= 3.0,
                    "{start}: {result:?}"
                );
            }
            assert!(result.warnings.is_empty());
        }
    }

    #[test]
    fn a_proportional_step_lands_in_one_move_and_an_accepted_start_takes_one_shot() {
        let mut meter = Linear::new([0.5 / 100.0, 0.5 / 100.0, 0.5 / 100.0]);
        let result = calibrate(&mut meter, [100; 3]).unwrap();
        assert_eq!(result.shots, [2; 3]);
        assert_eq!(result.levels, [186; 3]);
        let mut meter = Linear::new([0.93 / 100.0; 3]);
        let result = calibrate(&mut meter, [100; 3]).unwrap();
        assert_eq!(result.shots, [1; 3]);
        assert_eq!(result.levels, [100; 3]);
    }

    #[test]
    fn a_clipped_shot_halves_the_level() {
        let mut meter = Linear::new([0.01; 3]);
        let result = calibrate(&mut meter, [200, 1, 1]).unwrap();
        let red: Vec<u8> = meter
            .shots
            .iter()
            .filter(|(c, _)| *c == TripletChannel::Red)
            .map(|(_, l)| *l)
            .collect();
        assert_eq!(red[..2], [200, 100], "clipped at 200 halves to 100");
        assert!(ACCEPT.contains(&result.reached[0]));
    }

    #[test]
    fn levels_are_clamped_to_1_to_255_and_the_order_is_red_green_blue() {
        let mut meter = Linear::new([0.5, 0.5, 0.5]);
        let result = calibrate(&mut meter, [0, 0, 0]).unwrap();
        assert_eq!(meter.shots[0], (TripletChannel::Red, 1));
        assert!(result.levels.iter().all(|l| *l >= 1));
        let order: Vec<_> = meter.shots.iter().map(|(c, _)| *c).collect();
        let mut sorted = order.clone();
        sorted.sort_by_key(|c| c.index());
        assert_eq!(order, sorted);
    }

    #[test]
    fn a_step_too_coarse_to_land_inside_the_range_still_ends_below_clipping() {
        // Level 4 reads 0.8, level 5 reads 1.0 (clipped): nothing lies between.
        let mut meter = Linear::new([0.2; 3]);
        let result = calibrate(&mut meter, [2; 3]).unwrap();
        assert_eq!(result.levels, [4; 3]);
        assert!((result.reached[0] - 0.8).abs() < 1e-6);
        assert_eq!(
            result.warnings,
            [
                CalibrationWarning::CoarseLevel {
                    channel: TripletChannel::Red,
                    level: 4
                },
                CalibrationWarning::CoarseLevel {
                    channel: TripletChannel::Green,
                    level: 4
                },
                CalibrationWarning::CoarseLevel {
                    channel: TripletChannel::Blue,
                    level: 4
                },
            ]
        );
    }

    #[test]
    fn a_level_below_the_coarse_limit_warns_and_one_at_it_does_not() {
        let mut meter = Linear::new([0.93 / 19.0, 0.93 / 20.0, 0.93 / 255.0]);
        let result = calibrate(&mut meter, [19, 20, 255]).unwrap();
        assert_eq!(result.levels, [19, 20, 255]);
        assert_eq!(
            result.warnings,
            [CalibrationWarning::CoarseLevel {
                channel: TripletChannel::Red,
                level: 19
            }]
        );
    }

    /// Alternates dark and clipped whatever the level: it never settles.
    struct Flapping(u32);

    impl ChannelMeter for Flapping {
        fn measure(&mut self, _: TripletChannel, _: u8, _: u8) -> Result<f32, AppError> {
            self.0 += 1;
            Ok(if self.0 % 2 == 1 { 0.5 } else { 1.0 })
        }
    }

    #[test]
    fn a_colour_that_never_settles_stops_after_six_shots_and_says_so() {
        let mut meter = Flapping(0);
        let err = calibrate(&mut meter, [100; 3]).unwrap_err();
        assert_eq!(meter.0, 6);
        assert_eq!(
            err.message,
            "Red did not settle in 6 shots. Check that the film base fills the frame and nothing moved, then calibrate again."
        );
    }

    #[test]
    fn a_colour_clipped_at_level_1_fails_in_words() {
        // Halving from 44 reaches level 1 on the sixth shot. Red is fine; green clips all the way.
        let mut meter = Linear::new([0.02, 1.0, 0.02]);
        let err = calibrate(&mut meter, [44; 3]).unwrap_err();
        assert_eq!(
            err.message,
            "Green is clipped even at level 1: the exposure is too long. Use a shorter shutter speed and calibrate again."
        );
        let mut meter = Linear::new([2.0, 0.02, 0.02]);
        let err = calibrate(&mut meter, [44; 3]).unwrap_err();
        assert_eq!(
            err.message,
            "Red is clipped even at level 1: the exposure is too long. Use a shorter shutter speed and calibrate again."
        );
    }

    #[test]
    fn a_colour_too_dark_at_level_255_fails_in_words() {
        let mut meter = Linear::new([0.9 / 255.0 * 0.5, 0.9 / 255.0 * 0.5, 0.0001]);
        let err = calibrate(&mut meter, [100; 3]).unwrap_err();
        assert_eq!(
            err.message,
            "Red is too dark even at level 255: the exposure is too short. Use a longer shutter speed and calibrate again."
        );
        let mut meter = Linear::new([0.004, 0.004, 0.0001]);
        let err = calibrate(&mut meter, [100; 3]).unwrap_err();
        assert_eq!(
            err.message,
            "Blue is too dark even at level 255: the exposure is too short. Use a longer shutter speed and calibrate again."
        );
    }

    #[test]
    fn a_black_or_unreadable_reading_is_too_dark() {
        struct Fixed(f32);
        impl ChannelMeter for Fixed {
            fn measure(&mut self, _: TripletChannel, _: u8, _: u8) -> Result<f32, AppError> {
                Ok(self.0)
            }
        }
        for reading in [0.0, f32::NAN, -1.0] {
            let err = calibrate(&mut Fixed(reading), [10; 3]).unwrap_err();
            assert!(err.message.starts_with("Red is too dark even at level 255"));
        }
    }

    #[test]
    fn cancelling_stops_between_shots() {
        let mut meter = Linear::new([0.001; 3]);
        meter.cancel_after = Some(1);
        let err = calibrate(&mut meter, [10; 3]).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(meter.shots.len(), 1, "no shot after the cancel");
        let mut meter = Linear::new([0.001; 3]);
        meter.cancel_after = Some(0);
        assert_eq!(
            calibrate(&mut meter, [10; 3]).unwrap_err().code,
            ErrorCode::Cancelled
        );
        assert!(meter.shots.is_empty());
    }

    #[test]
    fn a_meter_error_ends_the_calibration() {
        struct Broken;
        impl ChannelMeter for Broken {
            fn measure(&mut self, _: TripletChannel, _: u8, _: u8) -> Result<f32, AppError> {
                Err(AppError::new(ErrorCode::Io, "no picture"))
            }
        }
        assert_eq!(
            calibrate(&mut Broken, [10; 3]).unwrap_err().message,
            "no picture"
        );
    }

    #[test]
    fn the_percentile_ignores_the_brightest_thousandth() {
        let mut values: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        values[0] = 1.0;
        // One bright pixel in a thousand (a speck) does not count.
        assert!((percentile(&mut values, PERCENTILE).unwrap() - 0.999).abs() < 1e-6);
        let mut two = vec![f32::NAN, 5.0, -3.0, 0.25];
        assert_eq!(percentile(&mut two, 1.0), Some(1.0));
        assert_eq!(percentile(&mut [], 0.5), None);
        assert_eq!(percentile(&mut [0.4], PERCENTILE), Some(0.4));
    }

    #[test]
    fn a_region_must_be_finite_sized_and_inside_the_frame() {
        let ok = Region {
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        };
        assert_eq!(ok.validated().unwrap(), ok);
        for bad in [
            Region { w: 0.0, ..ok },
            Region { h: -0.1, ..ok },
            Region { x: -0.1, ..ok },
            Region { x: 0.6, ..ok },
            Region { y: 0.51, ..ok },
            Region { x: f32::NAN, ..ok },
            Region {
                w: f32::INFINITY,
                ..ok
            },
        ] {
            assert_eq!(bad.validated().unwrap_err().code, ErrorCode::InvalidInput);
        }
        assert_eq!(ok.pixels(200, 100), (50, 25, 150, 75));
        let sliver = Region {
            x: 0.999,
            y: 0.0,
            w: 0.001,
            h: 1.0,
        };
        let (x0, _, x1, _) = sliver.pixels(10, 10);
        assert_eq!((x0, x1), (9, 10));
    }
}
