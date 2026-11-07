//! The light port: what the capture sequence needs from a light, whatever drives it.
//! `ScanlightV4` (serial) and `FakeLight` (tests) implement it (docs/specification.md section 6.1).

use gtp_core::AppError;
use serde::Serialize;

/// What `connect` found out about the light.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LightInfo {
    /// The serial port the light answered on.
    pub port: String,
    /// Hardware ID: 1 is the V4 rev A (PCB 26a901a), 2 the rev B (ADR 0004, amendment 3).
    pub hardware_id: u16,
    /// Firmware ID.
    pub firmware_id: u16,
    /// False when the hardware ID is not one this app knows (1 or 2). The light is still used,
    /// and a warning was logged.
    pub recognised: bool,
}

/// A serial port the light may be on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PortInfo {
    /// The port name to give to `connect` (`COM5`, `/dev/cu.usbmodem1101`).
    pub name: String,
    /// True for the Pico vendor ID of the V4: the port the probe tries first.
    pub likely: bool,
}

/// The last readings the light sent by itself (every 200 ms). Empty until the first arrives.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LightStatus {
    /// LED temperature in degrees Celsius.
    pub temp_c: Option<f32>,
    /// Supply voltage in millivolts.
    pub vbus_mv: Option<i32>,
    /// False once the link to the light failed (cable pulled, port closed).
    pub connected: bool,
}

/// A light that can be set to one colour at a time. Open loop: the device does not acknowledge
/// commands, so correctness is checked by the pictures that arrive.
pub trait LightSource {
    /// Opens the light and checks it answers `GET_FW_VERSION`. With no port given, the serial
    /// ports are probed.
    fn connect(&mut self, port: Option<&str>) -> Result<LightInfo, AppError>;
    /// `SET_COLOR` with these red, green and blue levels, white and IR off. The save flag is
    /// always 0: nothing is written to the light's memory.
    fn set_levels(&mut self, rgb: [u8; 3]) -> Result<(), AppError>;
    /// `SHUTTER_PULSE`, in units of 10 ms.
    fn shutter_pulse(&mut self, tens_of_ms: u8) -> Result<(), AppError>;
    /// The last `LED_TEMP` and `VBUS` readings.
    fn status(&self) -> LightStatus;
}
