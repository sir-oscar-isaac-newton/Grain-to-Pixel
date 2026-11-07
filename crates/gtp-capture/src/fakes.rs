//! A light for tests that records what it is told. Always compiled and public, like
//! `gtp_ingest::FixtureSource`, so the capture sequence and the application layer can test
//! against it without hardware.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use gtp_core::{AppError, ErrorCode};

use crate::camera::{CameraSource, CapturedFile};
use crate::light::{LightInfo, LightSource, LightStatus};

/// A command a [`FakeLight`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightCommand {
    /// `connect`, with the port asked for.
    Connect(Option<String>),
    /// `set_levels`. The dropped light records `SetLevels([0, 0, 0])` last, as the real driver
    /// sends a switch-off when it closes.
    SetLevels([u8; 3]),
    /// `shutter_pulse`, in units of 10 ms.
    ShutterPulse(u8),
}

#[derive(Default)]
struct State {
    commands: Vec<LightCommand>,
    status: LightStatus,
    fail: Option<AppError>,
}

/// A handle on the commands and the status of a [`FakeLight`], still usable after the light has
/// been moved into a `Box<dyn LightSource>` or dropped.
#[derive(Clone, Default)]
pub struct FakeLightHandle(Arc<Mutex<State>>);

impl FakeLightHandle {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every command so far, in order.
    pub fn commands(&self) -> Vec<LightCommand> {
        self.state().commands.clone()
    }

    /// The levels last set (all zeros if none, or after a switch-off).
    pub fn levels(&self) -> [u8; 3] {
        self.state()
            .commands
            .iter()
            .rev()
            .find_map(|command| match command {
                LightCommand::SetLevels(levels) => Some(*levels),
                _ => None,
            })
            .unwrap_or([0; 3])
    }

    /// What `status()` returns from now on (for temperature tests).
    pub fn set_status(&self, status: LightStatus) {
        self.state().status = status;
    }

    /// Makes `set_levels` and `shutter_pulse` fail with this error until cleared with `None`.
    /// The failed command is not recorded.
    pub fn fail_commands_with(&self, error: Option<AppError>) {
        self.state().fail = error;
    }
}

/// A light that records its commands. Dropping it records a switch-off, like the real driver.
pub struct FakeLight {
    handle: FakeLightHandle,
    info: LightInfo,
}

impl Default for FakeLight {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeLight {
    /// A connected-looking V4 rev A (hardware ID 1) at 25 °C.
    pub fn new() -> Self {
        let handle = FakeLightHandle::default();
        handle.set_status(LightStatus {
            temp_c: Some(25.0),
            vbus_mv: Some(5150),
            connected: true,
        });
        Self {
            handle,
            info: LightInfo {
                port: "fake".to_owned(),
                hardware_id: 1,
                firmware_id: 0,
                recognised: true,
            },
        }
    }

    /// A handle to read the commands from.
    pub fn handle(&self) -> FakeLightHandle {
        self.handle.clone()
    }
}

impl LightSource for FakeLight {
    fn connect(&mut self, port: Option<&str>) -> Result<LightInfo, AppError> {
        self.handle
            .state()
            .commands
            .push(LightCommand::Connect(port.map(str::to_owned)));
        Ok(self.info.clone())
    }

    fn set_levels(&mut self, rgb: [u8; 3]) -> Result<(), AppError> {
        self.record(LightCommand::SetLevels(rgb))
    }

    fn shutter_pulse(&mut self, tens_of_ms: u8) -> Result<(), AppError> {
        self.record(LightCommand::ShutterPulse(tens_of_ms))
    }

    fn status(&self) -> LightStatus {
        self.handle.state().status
    }
}

impl FakeLight {
    fn record(&mut self, command: LightCommand) -> Result<(), AppError> {
        let mut state = self.handle.state();
        if let Some(error) = &state.fail {
            return Err(error.clone());
        }
        state.commands.push(command);
        Ok(())
    }
}

impl Drop for FakeLight {
    fn drop(&mut self) {
        self.handle
            .state()
            .commands
            .push(LightCommand::SetLevels([0; 3]));
    }
}

/// The error a failing fake gives when none is chosen.
pub fn unplugged() -> AppError {
    AppError::new(ErrorCode::Io, "The light is not connected.")
}

/// What a [`FakeCamera`] does at one `wait_for_image`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shot {
    /// Delivers this picture.
    File(CapturedFile),
    /// Fails with this error (a timeout, a camera error).
    Fail(AppError),
}

/// A call a [`FakeCamera`] received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraCall {
    /// `arm`.
    Arm,
    /// `trigger`, with the light levels at that moment when the camera is linked to a light.
    Trigger { levels: Option<[u8; 3]> },
    /// `wait_for_image`, with the timeout asked for.
    Wait(Duration),
}

type WaitHook = Box<dyn FnMut(usize) + Send>;

#[derive(Default)]
struct CameraState {
    shots: VecDeque<Shot>,
    calls: Vec<CameraCall>,
    connected: bool,
    fail_arm: Option<AppError>,
    fail_trigger: Option<AppError>,
    light: Option<FakeLightHandle>,
    waits: usize,
    on_wait: Option<WaitHook>,
}

/// A handle on a [`FakeCamera`], still usable after the camera was moved into a `Box`.
#[derive(Clone, Default)]
pub struct FakeCameraHandle(Arc<Mutex<CameraState>>);

impl FakeCameraHandle {
    fn state(&self) -> MutexGuard<'_, CameraState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every call so far, in order.
    pub fn calls(&self) -> Vec<CameraCall> {
        self.state().calls.clone()
    }

    /// What `is_connected` returns from now on.
    pub fn set_connected(&self, connected: bool) {
        self.state().connected = connected;
    }

    /// Makes `arm` fail until cleared with `None`.
    pub fn fail_arm_with(&self, error: Option<AppError>) {
        self.state().fail_arm = error;
    }

    /// Makes `trigger` fail until cleared with `None`.
    pub fn fail_trigger_with(&self, error: Option<AppError>) {
        self.state().fail_trigger = error;
    }
}

/// A camera that delivers scripted pictures and records its calls. With no shots left it fails
/// like a camera that never answered.
pub struct FakeCamera {
    handle: FakeCameraHandle,
}

impl FakeCamera {
    /// A connected camera that plays `shots`, one per `wait_for_image`.
    pub fn new(shots: Vec<Shot>) -> Self {
        let handle = FakeCameraHandle::default();
        {
            let mut state = handle.state();
            state.connected = true;
            state.shots = shots.into();
        }
        Self { handle }
    }

    /// A connected camera that delivers `files`, one per shot.
    pub fn with_files(files: Vec<CapturedFile>) -> Self {
        Self::new(files.into_iter().map(Shot::File).collect())
    }

    /// Records the light's levels at each `trigger`.
    #[must_use]
    pub fn linked_to(self, light: FakeLightHandle) -> Self {
        self.handle.state().light = Some(light);
        self
    }

    /// Calls `hook` with the number of the shot (from 0) at the start of each `wait_for_image`,
    /// before its result is chosen: a test can change the light's status or fail its commands at
    /// exactly that point of the sequence. The hook must not call this camera's handle.
    #[must_use]
    pub fn on_wait(self, hook: impl FnMut(usize) + Send + 'static) -> Self {
        self.handle.state().on_wait = Some(Box::new(hook));
        self
    }

    /// A handle to read the calls from.
    pub fn handle(&self) -> FakeCameraHandle {
        self.handle.clone()
    }
}

impl CameraSource for FakeCamera {
    fn is_connected(&self) -> bool {
        self.handle.state().connected
    }

    fn arm(&mut self) -> Result<(), AppError> {
        let mut state = self.handle.state();
        state.calls.push(CameraCall::Arm);
        state.fail_arm.clone().map_or(Ok(()), Err)
    }

    fn trigger(&mut self) -> Result<(), AppError> {
        let mut state = self.handle.state();
        let levels = state.light.as_ref().map(FakeLightHandle::levels);
        state.calls.push(CameraCall::Trigger { levels });
        state.fail_trigger.clone().map_or(Ok(()), Err)
    }

    fn wait_for_image(&mut self, timeout: Duration) -> Result<CapturedFile, AppError> {
        let (shot, hook) = {
            let mut state = self.handle.state();
            state.calls.push(CameraCall::Wait(timeout));
            let shot = state.waits;
            state.waits += 1;
            (shot, state.on_wait.take())
        };
        if let Some(mut hook) = hook {
            hook(shot);
            self.handle.state().on_wait = Some(hook);
        }
        match self.handle.state().shots.pop_front() {
            Some(Shot::File(file)) => Ok(file),
            Some(Shot::Fail(error)) => Err(error),
            None => Err(no_picture()),
        }
    }
}

/// The error a camera gives when no picture came in time.
pub fn no_picture() -> AppError {
    AppError::new(
        ErrorCode::Io,
        "The camera did not deliver a picture in time.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fake_records_commands_in_order_and_switches_off_when_dropped() {
        let mut light = FakeLight::new();
        let handle = light.handle();
        light.connect(Some("COM5")).unwrap();
        light.set_levels([44, 0, 0]).unwrap();
        assert_eq!(handle.levels(), [44, 0, 0]);
        light.shutter_pulse(10).unwrap();
        drop(light);
        assert_eq!(
            handle.commands(),
            [
                LightCommand::Connect(Some("COM5".into())),
                LightCommand::SetLevels([44, 0, 0]),
                LightCommand::ShutterPulse(10),
                LightCommand::SetLevels([0, 0, 0]),
            ]
        );
        assert_eq!(handle.levels(), [0; 3]);
    }

    #[test]
    fn a_failing_fake_returns_the_error_and_records_nothing() {
        let mut light = FakeLight::new();
        let handle = light.handle();
        handle.fail_commands_with(Some(unplugged()));
        assert!(light.set_levels([1, 2, 3]).is_err());
        assert!(handle.commands().is_empty());
        handle.fail_commands_with(None);
        assert!(light.set_levels([1, 2, 3]).is_ok());
    }

    #[test]
    fn the_status_can_be_scripted() {
        let light = FakeLight::new();
        light.handle().set_status(LightStatus {
            temp_c: Some(80.0),
            ..light.status()
        });
        assert_eq!(light.status().temp_c, Some(80.0));
    }
}
