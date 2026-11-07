//! The per-frame R, G, B capture sequence and the hardware-safety rules around it
//! (docs/specification.md section 6.1, ADR 0004 amendments 4 and 5).
//!
//! Per channel, in the order red, green, blue: set only that colour's level, wait the settle time,
//! check the light, arm, trigger, wait for the picture. The light is open loop, so the pictures
//! arriving are the check that it did what it was told. The light is switched off (all zeros, never
//! saved) on every way out: success, cancel, error, a camera timeout, a temperature abort and a
//! panic, by a guard whose `Drop` does it.

use std::time::Duration;

use gtp_core::{AppError, ErrorCode, TripletChannel};

use crate::camera::{CameraSource, CapturedFile};
use crate::light::{LightSource, LightStatus};
use crate::profile::{CaptureProfile, VBUS_WARN_MV};

/// How long the camera gets to deliver one picture.
pub const CAPTURE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long a start waits for the light's first temperature reading (it sends one every 200 ms).
pub const TEMP_READING_WAIT: Duration = Duration::from_secs(1);
/// A wait is made of pauses of at most this long, with the cancel flag checked between them.
const WAIT_SLICE: Duration = Duration::from_millis(50);

/// The clock of the sequence: tests replace it so that no test really waits.
pub trait Pause {
    /// Waits for `duration` (the sequence asks for at most 50 ms at a time).
    fn pause(&self, duration: Duration);
}

/// The real clock: sleeps the thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadPause;

impl Pause for ThreadPause {
    fn pause(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Something to know about a finished sequence. Not an error: the three pictures were taken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaptureWarning {
    /// The LED temperature went above the warn limit; this is the highest reading seen.
    HighTemperature { temp_c: f32 },
    /// The supply voltage went below [`VBUS_WARN_MV`]; this is the lowest reading seen.
    LowSupply { vbus_mv: i32 },
}

/// What a finished sequence gives back.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureOutcome {
    /// The red-, green- and blue-lit picture, in that order.
    pub files: [CapturedFile; 3],
    /// Temperature and supply warnings, each kind at most once.
    pub warnings: Vec<CaptureWarning>,
}

/// Switches the light off when dropped, whichever way the sequence ends (including a panic). A
/// failure of the off command is logged and nothing else: it must not hide the error that ended
/// the sequence, and there is nothing more to try.
struct LightGuard<'a> {
    light: &'a mut dyn LightSource,
}

impl Drop for LightGuard<'_> {
    fn drop(&mut self) {
        if let Err(err) = self.light.set_levels([0; 3]) {
            tracing::error!("switching the light off failed: {err}");
        }
    }
}

/// Runs one capture. Settings come from the [`CaptureProfile`]; the clock and the two limits that
/// tests shorten are fields with the real values as defaults.
pub struct CaptureSequence<'a> {
    profile: CaptureProfile,
    pause: &'a dyn Pause,
    reading_wait: Duration,
    capture_timeout: Duration,
}

/// The real clock, for [`CaptureSequence::new`].
static THREAD_PAUSE: ThreadPause = ThreadPause;

impl CaptureSequence<'static> {
    /// A sequence with the real clock and the real limits.
    pub fn new(profile: CaptureProfile) -> Self {
        Self::with_pause(profile, &THREAD_PAUSE)
    }
}

impl<'a> CaptureSequence<'a> {
    /// A sequence that waits with `pause`.
    pub fn with_pause(profile: CaptureProfile, pause: &'a dyn Pause) -> Self {
        Self {
            profile,
            pause,
            reading_wait: TEMP_READING_WAIT,
            capture_timeout: CAPTURE_TIMEOUT,
        }
    }

    /// How long a start waits for a first temperature reading.
    #[must_use]
    pub fn reading_wait(mut self, wait: Duration) -> Self {
        self.reading_wait = wait;
        self
    }

    /// How long the camera gets for each picture.
    #[must_use]
    pub fn capture_timeout(mut self, timeout: Duration) -> Self {
        self.capture_timeout = timeout;
        self
    }

    /// Takes the three pictures. `cancelled` is asked between all steps and during every wait, and
    /// ends the sequence with `Cancelled`; `on_step` is called with each channel once its picture
    /// is in. The light is off when this returns or unwinds.
    pub fn run(
        &self,
        light: &mut dyn LightSource,
        camera: &mut dyn CameraSource,
        cancelled: &dyn Fn() -> bool,
        on_step: &mut dyn FnMut(TripletChannel),
    ) -> Result<CaptureOutcome, AppError> {
        let guard = LightGuard { light };
        let mut warnings = Warnings::default();

        if !guard.light.status().connected {
            return Err(light_lost());
        }
        if !camera.is_connected() {
            return Err(camera_lost());
        }
        self.wait_for_reading(&*guard.light, cancelled)?;
        // Before anything is lit: a light already over the abort limit stays off.
        self.check_light(guard.light.status(), &mut warnings)?;

        let mut files = Vec::with_capacity(3);
        for (channel, level) in TripletChannel::ALL.into_iter().zip(self.profile.levels) {
            check(cancelled)?;
            if !guard.light.status().connected {
                return Err(light_lost());
            }
            let mut levels = [0u8; 3];
            levels[channel_index(channel)] = level;
            guard.light.set_levels(levels)?;
            self.settle(cancelled)?;
            self.check_light(guard.light.status(), &mut warnings)?;
            check(cancelled)?;
            if !camera.is_connected() {
                return Err(camera_lost());
            }
            camera.arm()?;
            check(cancelled)?;
            camera.trigger()?;
            check(cancelled)?;
            files.push(camera.wait_for_image(self.capture_timeout)?);
            on_step(channel);
        }
        let files = <[CapturedFile; 3]>::try_from(files)
            .map_err(|_| AppError::internal("the sequence did not produce three pictures"))?;
        Ok(CaptureOutcome {
            files,
            warnings: warnings.into_vec(),
        })
    }

    /// The settle wait, in short pauses so a cancel is seen within one of them.
    fn settle(&self, cancelled: &dyn Fn() -> bool) -> Result<(), AppError> {
        let mut left = Duration::from_millis(u64::from(self.profile.settle_ms));
        while !left.is_zero() {
            check(cancelled)?;
            let slice = left.min(WAIT_SLICE);
            self.pause.pause(slice);
            left -= slice;
        }
        check(cancelled)
    }

    /// Waits for the light's first temperature reading; a light that sends none is not used,
    /// since the temperature limit could not be kept.
    fn wait_for_reading(
        &self,
        light: &dyn LightSource,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), AppError> {
        let mut left = self.reading_wait;
        loop {
            if reading(light.status()).is_some() {
                return Ok(());
            }
            if left.is_zero() {
                return Err(AppError::new(
                    ErrorCode::Io,
                    "The light is not reporting its temperature, so it cannot be used safely. Unplug its USB cable, plug it in again, reconnect it in the app and start the frame again.",
                ));
            }
            check(cancelled)?;
            let slice = left.min(WAIT_SLICE);
            self.pause.pause(slice);
            left -= slice;
        }
    }

    /// The safety checks before an exposure: link up, a reading present, under the abort limit;
    /// notes the warnings.
    fn check_light(&self, status: LightStatus, warnings: &mut Warnings) -> Result<(), AppError> {
        if !status.connected {
            return Err(light_lost());
        }
        let Some(temp_c) = reading(status) else {
            return Err(AppError::new(
                ErrorCode::Io,
                "The light stopped reporting its temperature, so it cannot be used safely. The light was switched off. Unplug its USB cable, plug it in again, reconnect it in the app and start the frame again.",
            ));
        };
        if temp_c > self.profile.temp_abort_c {
            return Err(AppError::new(
                ErrorCode::Io,
                format!(
                    "The light is too hot ({temp_c:.0} C, the limit is {:.0} C) and was switched off. Let it cool down, then start the frame again.",
                    self.profile.temp_abort_c
                ),
            ));
        }
        if temp_c > self.profile.temp_warn_c {
            warnings.temperature(temp_c);
        }
        if let Some(vbus_mv) = status.vbus_mv.filter(|mv| *mv < VBUS_WARN_MV) {
            warnings.supply(vbus_mv);
        }
        Ok(())
    }
}

/// The temperature, when the light gave a usable one.
fn reading(status: LightStatus) -> Option<f32> {
    status.temp_c.filter(|t| t.is_finite())
}

fn channel_index(channel: TripletChannel) -> usize {
    match channel {
        TripletChannel::Red => 0,
        TripletChannel::Green => 1,
        TripletChannel::Blue => 2,
    }
}

fn check(cancelled: &dyn Fn() -> bool) -> Result<(), AppError> {
    if cancelled() {
        Err(AppError::new(
            ErrorCode::Cancelled,
            "The capture was cancelled.",
        ))
    } else {
        Ok(())
    }
}

fn light_lost() -> AppError {
    AppError::new(
        ErrorCode::Io,
        "The light is not connected. Check the Scanlight's USB cable, reconnect it in the app and start the frame again.",
    )
}

fn camera_lost() -> AppError {
    AppError::new(
        ErrorCode::Io,
        "The camera is not connected. Check its USB cable and that it is switched on, then start the frame again.",
    )
}

/// The worst temperature and supply readings seen.
#[derive(Default)]
struct Warnings {
    temp_c: Option<f32>,
    vbus_mv: Option<i32>,
}

impl Warnings {
    fn temperature(&mut self, temp_c: f32) {
        self.temp_c = Some(self.temp_c.map_or(temp_c, |seen| seen.max(temp_c)));
    }

    fn supply(&mut self, vbus_mv: i32) {
        self.vbus_mv = Some(self.vbus_mv.map_or(vbus_mv, |seen| seen.min(vbus_mv)));
    }

    fn into_vec(self) -> Vec<CaptureWarning> {
        let mut all = Vec::new();
        if let Some(temp_c) = self.temp_c {
            all.push(CaptureWarning::HighTemperature { temp_c });
        }
        if let Some(vbus_mv) = self.vbus_mv {
            all.push(CaptureWarning::LowSupply { vbus_mv });
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use super::*;
    use crate::fakes::{
        no_picture, unplugged, CameraCall, FakeCamera, FakeLight, FakeLightHandle, LightCommand,
        Shot,
    };

    /// A clock that adds up the waits instead of waiting.
    #[derive(Default)]
    struct VirtualClock {
        waited: Cell<Duration>,
        pauses: Cell<u32>,
        /// Cancels once this many pauses were made.
        cancel_after: Cell<Option<u32>>,
    }

    impl Pause for VirtualClock {
        fn pause(&self, duration: Duration) {
            assert!(duration <= WAIT_SLICE, "a blind long sleep: {duration:?}");
            self.waited.set(self.waited.get() + duration);
            self.pauses.set(self.pauses.get() + 1);
        }
    }

    impl VirtualClock {
        fn cancelled(&self) -> bool {
            self.cancel_after
                .get()
                .is_some_and(|after| self.pauses.get() >= after)
        }
    }

    fn file(n: u8) -> CapturedFile {
        CapturedFile {
            name: format!("DSC_{n:04}.NEF"),
            bytes: vec![n; 4],
        }
    }

    fn three_files() -> Vec<CapturedFile> {
        vec![file(1), file(2), file(3)]
    }

    fn status(temp_c: Option<f32>, vbus_mv: Option<i32>) -> LightStatus {
        LightStatus {
            temp_c,
            vbus_mv,
            connected: true,
        }
    }

    struct Rig {
        light: FakeLight,
        light_handle: FakeLightHandle,
        clock: VirtualClock,
    }

    impl Rig {
        fn new() -> Self {
            let light = FakeLight::new();
            Self {
                light_handle: light.handle(),
                light,
                clock: VirtualClock::default(),
            }
        }

        fn camera(&self) -> FakeCamera {
            FakeCamera::with_files(three_files()).linked_to(self.light_handle.clone())
        }

        fn run(&mut self, camera: &mut dyn CameraSource) -> Result<CaptureOutcome, AppError> {
            let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &self.clock);
            let clock = &self.clock;
            sequence.run(&mut self.light, camera, &|| clock.cancelled(), &mut |_| {})
        }

        /// The levels the light was set to, in order.
        fn levels(&self) -> Vec<[u8; 3]> {
            self.light_handle
                .commands()
                .into_iter()
                .filter_map(|c| match c {
                    LightCommand::SetLevels(levels) => Some(levels),
                    _ => None,
                })
                .collect()
        }

        fn assert_off(&self) {
            assert_eq!(
                self.levels().last(),
                Some(&[0, 0, 0]),
                "{:?}",
                self.levels()
            );
        }

        fn never_lit(&self) -> bool {
            self.levels().iter().all(|l| *l == [0; 3])
        }
    }

    #[test]
    fn the_channels_go_red_green_blue_each_lit_alone_at_its_own_level() {
        let mut rig = Rig::new();
        let mut camera = rig.camera();
        let handle = camera.handle();
        let mut steps = Vec::new();
        let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &rig.clock);
        let outcome = sequence
            .run(&mut rig.light, &mut camera, &|| false, &mut |c| {
                steps.push(c)
            })
            .unwrap();
        assert_eq!(outcome.files.to_vec(), three_files());
        assert!(outcome.warnings.is_empty());
        assert_eq!(steps, TripletChannel::ALL);
        assert_eq!(rig.levels(), [[44, 0, 0], [0, 22, 0], [0, 0, 2], [0, 0, 0]]);
        let at = |levels| CameraCall::Trigger {
            levels: Some(levels),
        };
        let wait = CameraCall::Wait(CAPTURE_TIMEOUT);
        assert_eq!(
            handle.calls(),
            [
                CameraCall::Arm,
                at([44, 0, 0]),
                wait.clone(),
                CameraCall::Arm,
                at([0, 22, 0]),
                wait.clone(),
                CameraCall::Arm,
                at([0, 0, 2]),
                wait,
            ]
        );
        // The sequence only sets colours: no shutter pulse (the camera is triggered over USB).
        assert!(!rig
            .light_handle
            .commands()
            .iter()
            .any(|c| matches!(c, LightCommand::ShutterPulse(_))));
        assert_eq!(CAPTURE_TIMEOUT, Duration::from_secs(15));
    }

    #[test]
    fn the_settle_time_is_waited_once_per_channel_in_short_pauses() {
        let mut rig = Rig::new();
        let mut camera = rig.camera();
        rig.run(&mut camera).unwrap();
        assert_eq!(rig.clock.waited.get(), Duration::from_millis(3 * 500));

        let mut other = Rig::new();
        let mut camera = other.camera();
        let profile = CaptureProfile {
            settle_ms: 120,
            ..CaptureProfile::default()
        };
        let sequence = CaptureSequence::with_pause(profile, &other.clock);
        sequence
            .run(&mut other.light, &mut camera, &|| false, &mut |_| {})
            .unwrap();
        assert_eq!(other.clock.waited.get(), Duration::from_millis(360));
    }

    #[test]
    fn the_wait_for_the_picture_uses_the_capture_timeout() {
        let mut rig = Rig::new();
        let mut camera = rig.camera();
        let handle = camera.handle();
        let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &rig.clock)
            .capture_timeout(Duration::from_millis(7));
        sequence
            .run(&mut rig.light, &mut camera, &|| false, &mut |_| {})
            .unwrap();
        assert!(handle
            .calls()
            .contains(&CameraCall::Wait(Duration::from_millis(7))));
    }

    #[test]
    fn the_light_is_off_after_success() {
        let mut rig = Rig::new();
        let mut camera = rig.camera();
        rig.run(&mut camera).unwrap();
        rig.assert_off();
    }

    #[test]
    fn the_light_is_off_after_a_cancel_before_the_start_and_during_a_settle() {
        let mut rig = Rig::new();
        let mut camera = rig.camera();
        let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &rig.clock);
        let err = sequence
            .run(&mut rig.light, &mut camera, &|| true, &mut |_| {})
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        rig.assert_off();
        assert!(camera.handle().calls().is_empty());

        // Ten pauses make the red settle; the cancel comes after five more, in the green settle.
        let mut rig = Rig::new();
        rig.clock.cancel_after.set(Some(15));
        let mut camera = rig.camera();
        let handle = camera.handle();
        let err = rig.run(&mut camera).unwrap_err();
        assert_eq!(err.code, ErrorCode::Cancelled);
        assert_eq!(rig.levels(), [[44, 0, 0], [0, 22, 0], [0, 0, 0]]);
        assert_eq!(handle.calls().len(), 3, "only the red shot was taken");
        assert_eq!(rig.clock.pauses.get(), 15, "the wait stopped at the cancel");
    }

    #[test]
    fn a_cancel_between_arm_trigger_and_wait_is_seen() {
        for stop_at in 0..3 {
            let mut rig = Rig::new();
            let mut camera = rig.camera();
            let handle = camera.handle();
            let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &rig.clock);
            let asked = Cell::new(0);
            // Count the cancel checks from the first settle on; cancel at the chosen camera step.
            let cancelled = || {
                let calls = handle.calls().len();
                asked.set(asked.get() + 1);
                calls > stop_at && calls > 0
            };
            let err = sequence
                .run(&mut rig.light, &mut camera, &cancelled, &mut |_| {})
                .unwrap_err();
            assert_eq!(err.code, ErrorCode::Cancelled, "stop_at {stop_at}");
            assert!(handle.calls().len() <= 3, "stop_at {stop_at}");
            rig.assert_off();
        }
    }

    #[test]
    fn the_light_is_off_after_a_camera_error_and_a_camera_timeout() {
        let mut rig = Rig::new();
        let mut camera = FakeCamera::new(vec![
            Shot::File(file(1)),
            Shot::Fail(AppError::new(ErrorCode::Io, "camera busy")),
        ]);
        let err = rig.run(&mut camera).unwrap_err();
        assert_eq!(err.message, "camera busy");
        assert_eq!(rig.levels(), [[44, 0, 0], [0, 22, 0], [0, 0, 0]]);

        let mut rig = Rig::new();
        let mut camera = FakeCamera::new(vec![Shot::File(file(1)), Shot::Fail(no_picture())]);
        let err = rig.run(&mut camera).unwrap_err();
        assert_eq!(err, no_picture());
        rig.assert_off();

        for fail_arm in [true, false] {
            let mut rig = Rig::new();
            let mut camera = rig.camera();
            let broken = Some(AppError::new(ErrorCode::Io, "refused"));
            if fail_arm {
                camera.handle().fail_arm_with(broken);
            } else {
                camera.handle().fail_trigger_with(broken);
            }
            assert_eq!(rig.run(&mut camera).unwrap_err().message, "refused");
            assert_eq!(rig.levels(), [[44, 0, 0], [0, 0, 0]]);
        }
    }

    /// A camera that panics at its second picture.
    struct PanickingCamera(u8);

    impl CameraSource for PanickingCamera {
        fn is_connected(&self) -> bool {
            true
        }
        fn arm(&mut self) -> Result<(), AppError> {
            Ok(())
        }
        fn trigger(&mut self) -> Result<(), AppError> {
            Ok(())
        }
        fn wait_for_image(&mut self, _: Duration) -> Result<CapturedFile, AppError> {
            self.0 += 1;
            assert!(self.0 < 2, "the camera driver crashed");
            Ok(file(self.0))
        }
    }

    #[test]
    fn the_light_is_off_after_a_panic() {
        let mut rig = Rig::new();
        let mut camera = PanickingCamera(0);
        let caught = catch_unwind(AssertUnwindSafe(|| rig.run(&mut camera)));
        assert!(caught.is_err());
        assert_eq!(rig.levels(), [[44, 0, 0], [0, 22, 0], [0, 0, 0]]);
    }

    #[test]
    fn a_light_already_over_the_abort_limit_is_never_lit() {
        let mut rig = Rig::new();
        rig.light_handle.set_status(status(Some(55.5), Some(5150)));
        let mut camera = rig.camera();
        let err = rig.run(&mut camera).unwrap_err();
        assert!(err.message.contains("too hot"), "{err}");
        assert!(rig.levels().iter().all(|levels| *levels == [0, 0, 0]));
        rig.assert_off();
    }

    #[test]
    fn a_temperature_above_the_abort_limit_stops_the_sequence_and_switches_the_light_off() {
        let mut rig = Rig::new();
        let heat = rig.light_handle.clone();
        // Fine for the red shot, too hot by the green one.
        let mut camera = rig.camera().on_wait(move |shot| {
            if shot == 0 {
                heat.set_status(status(Some(55.5), Some(5150)));
            }
        });
        let handle = camera.handle();
        let err = rig.run(&mut camera).unwrap_err();
        assert!(err.message.contains("too hot"), "{err}");
        assert!(err.message.contains("cool down"), "{err}");
        assert_eq!(rig.levels(), [[44, 0, 0], [0, 22, 0], [0, 0, 0]]);
        assert_eq!(handle.calls().len(), 3, "no second shot was armed or fired");
    }

    #[test]
    fn a_temperature_already_over_the_abort_limit_never_takes_a_shot() {
        let mut rig = Rig::new();
        rig.light_handle.set_status(status(Some(60.0), Some(5150)));
        let mut camera = rig.camera();
        let handle = camera.handle();
        assert!(rig.run(&mut camera).is_err());
        assert!(handle.calls().is_empty());
        rig.assert_off();
    }

    #[test]
    fn a_temperature_exactly_at_the_abort_limit_is_not_over_it() {
        let mut rig = Rig::new();
        rig.light_handle.set_status(status(Some(55.0), Some(5150)));
        let mut camera = rig.camera();
        let outcome = rig.run(&mut camera).unwrap();
        assert_eq!(
            outcome.warnings,
            [CaptureWarning::HighTemperature { temp_c: 55.0 }]
        );
    }

    #[test]
    fn a_temperature_above_the_warn_limit_warns_once_with_the_highest_and_goes_on() {
        let mut rig = Rig::new();
        // Already warm at the start: the warn limit is exceeded before the first shot.
        rig.light_handle.set_status(status(Some(46.0), Some(5150)));
        let hotter = rig.light_handle.clone();
        let mut camera = rig.camera().on_wait(move |shot| {
            hotter.set_status(status(Some(48.0 + shot as f32), Some(5150)));
        });
        let outcome = rig.run(&mut camera).unwrap();
        assert_eq!(outcome.files.to_vec(), three_files());
        // Checked before shots 0, 1 and 2: 46, 48, 49.
        assert_eq!(
            outcome.warnings,
            [CaptureWarning::HighTemperature { temp_c: 49.0 }]
        );
        rig.assert_off();
    }

    #[test]
    fn a_low_supply_warns_with_the_lowest_reading() {
        let mut rig = Rig::new();
        rig.light_handle.set_status(status(Some(25.0), Some(4740)));
        let light = rig.light_handle.clone();
        let mut camera = rig.camera().on_wait(move |shot| {
            let vbus_mv = [4700, 4600, 4800][shot.min(2)];
            light.set_status(status(Some(25.0), Some(vbus_mv)));
        });
        let outcome = rig.run(&mut camera).unwrap();
        assert_eq!(
            outcome.warnings,
            [CaptureWarning::LowSupply { vbus_mv: 4600 }]
        );
    }

    #[test]
    fn a_supply_at_the_limit_does_not_warn() {
        let mut rig = Rig::new();
        rig.light_handle
            .set_status(status(Some(25.0), Some(VBUS_WARN_MV)));
        let mut camera = rig.camera();
        assert!(rig.run(&mut camera).unwrap().warnings.is_empty());
    }

    #[test]
    fn with_no_temperature_reading_it_refuses_to_start_after_the_wait() {
        let mut rig = Rig::new();
        rig.light_handle.set_status(status(None, Some(5150)));
        let mut camera = rig.camera();
        let handle = camera.handle();
        let err = rig.run(&mut camera).unwrap_err();
        assert!(err.message.contains("temperature"), "{err}");
        assert!(err.message.contains("plug it in again"), "{err}");
        assert_eq!(rig.clock.waited.get(), TEMP_READING_WAIT);
        assert!(handle.calls().is_empty());
        assert!(rig.never_lit());
    }

    #[test]
    fn the_reading_wait_can_be_shortened() {
        let mut rig = Rig::new();
        rig.light_handle.set_status(status(None, None));
        let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &rig.clock)
            .reading_wait(Duration::from_millis(100));
        let mut camera = rig.camera();
        assert!(sequence
            .run(&mut rig.light, &mut camera, &|| false, &mut |_| {})
            .is_err());
        assert_eq!(rig.clock.waited.get(), Duration::from_millis(100));
    }

    #[test]
    fn a_reading_that_arrives_during_the_wait_lets_it_start() {
        let mut rig = Rig::new();
        rig.light_handle.set_status(status(None, None));
        let light = rig.light_handle.clone();
        let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &rig.clock);
        let mut camera = rig.camera();
        let clock = &rig.clock;
        // After the third pause the light's first reading comes in.
        let outcome = sequence.run(
            &mut rig.light,
            &mut camera,
            &|| {
                if clock.pauses.get() == 3 {
                    light.set_status(status(Some(25.0), Some(5150)));
                }
                false
            },
            &mut |_| {},
        );
        assert!(outcome.is_ok(), "{outcome:?}");
    }

    #[test]
    fn readings_that_vanish_mid_sequence_abort_it() {
        let mut rig = Rig::new();
        let light = rig.light_handle.clone();
        let mut camera = rig
            .camera()
            .on_wait(move |_| light.set_status(status(None, None)));
        let err = rig.run(&mut camera).unwrap_err();
        assert!(err.message.contains("stopped reporting"), "{err}");
        assert_eq!(rig.levels(), [[44, 0, 0], [0, 22, 0], [0, 0, 0]]);
    }

    #[test]
    fn a_link_lost_mid_sequence_aborts_it() {
        let mut rig = Rig::new();
        let light = rig.light_handle.clone();
        let mut camera = rig.camera().on_wait(move |shot| {
            if shot == 1 {
                light.set_status(LightStatus::default());
            }
        });
        let handle = camera.handle();
        let err = rig.run(&mut camera).unwrap_err();
        assert!(err.message.contains("not connected"), "{err}");
        // Red and green were taken; blue was never lit.
        assert_eq!(handle.calls().len(), 6);
        assert_eq!(rig.levels(), [[44, 0, 0], [0, 22, 0], [0, 0, 0]]);
    }

    #[test]
    fn a_light_or_camera_that_is_not_connected_refuses_to_start() {
        let mut rig = Rig::new();
        rig.light_handle.set_status(LightStatus::default());
        let mut camera = rig.camera();
        let err = rig.run(&mut camera).unwrap_err();
        assert!(err.message.contains("light is not connected"), "{err}");
        assert!(camera.handle().calls().is_empty());

        let mut rig = Rig::new();
        let mut camera = rig.camera();
        camera.handle().set_connected(false);
        let err = rig.run(&mut camera).unwrap_err();
        assert!(err.message.contains("camera is not connected"), "{err}");
        assert!(rig.never_lit());
    }

    #[test]
    fn a_camera_that_goes_away_mid_sequence_stops_it() {
        let mut rig = Rig::new();
        let mut camera = rig.camera();
        let handle = camera.handle();
        let mut steps = 0;
        let sequence = CaptureSequence::with_pause(CaptureProfile::default(), &rig.clock);
        let err = sequence
            .run(&mut rig.light, &mut camera, &|| false, &mut |_| {
                steps += 1;
                handle.set_connected(false);
            })
            .unwrap_err();
        assert_eq!(steps, 1);
        assert!(err.message.contains("camera is not connected"), "{err}");
        rig.assert_off();
    }

    #[test]
    fn a_failing_off_command_does_not_hide_the_error_that_ended_the_sequence() {
        let mut rig = Rig::new();
        let light = rig.light_handle.clone();
        // The camera fails and, at the same moment, the light's commands start failing.
        let mut camera = FakeCamera::new(vec![Shot::Fail(no_picture())])
            .on_wait(move |_| light.fail_commands_with(Some(unplugged())));
        let err = rig.run(&mut camera).unwrap_err();
        assert_eq!(err, no_picture());
        // The off command failed and was not recorded: the light was set to red and left.
        assert_eq!(rig.levels(), [[44, 0, 0]]);
    }

    #[test]
    fn a_failing_set_levels_ends_the_sequence_with_its_error() {
        let mut rig = Rig::new();
        rig.light_handle.fail_commands_with(Some(unplugged()));
        let mut camera = rig.camera();
        assert_eq!(rig.run(&mut camera).unwrap_err(), unplugged());
    }
}
