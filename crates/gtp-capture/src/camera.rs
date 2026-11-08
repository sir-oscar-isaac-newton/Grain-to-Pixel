//! The camera port: what the capture sequence needs from a camera, whatever drives it.
//! `FakeCamera` (tests) and `GphotoCamera` (libgphoto2, macOS, H-09) implement it; the watched-folder
//! backend comes with H-10 (docs/specification.md section 6.1, ADR 0004 amendment 2).

use std::time::Duration;

use gtp_core::AppError;
use serde::Serialize;

/// One picture the camera delivered: its file name and its bytes. Nothing is decoded here; the
/// merge path reads the EXIF data and the sensor data from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFile {
    /// The file name the camera or its tool gave the picture (no folder).
    pub name: String,
    /// The NEF bytes.
    pub bytes: Vec<u8>,
}

/// A camera the app can use, as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CameraInfo {
    /// A stable id for the camera.
    pub id: String,
    /// The name to show.
    pub name: String,
}

/// A camera that takes one picture on request. The camera is triggered over USB, not through the
/// light, so `trigger` needs no light.
pub trait CameraSource {
    /// False once the link to the camera is gone (cable pulled, camera off).
    fn is_connected(&self) -> bool;
    /// Gets ready to receive one exposure.
    fn arm(&mut self) -> Result<(), AppError>;
    /// Takes the picture (a PTP capture, or a command to the tool that does).
    fn trigger(&mut self) -> Result<(), AppError>;
    /// Waits up to `timeout` for the picture and returns it. An error when none arrived in time.
    fn wait_for_image(&mut self, timeout: Duration) -> Result<CapturedFile, AppError>;
}

/// Finds the camera a backend drives, before a job needs it, so that a camera plugged in after the
/// app started is found when it is first needed. Kept apart from [`CameraSource`] because a job
/// holds the source for its whole run while the UI may still ask for the list.
pub trait CameraFinder: Send + Sync {
    /// The cameras plugged in now. Asking again is also how the user asks for a fresh look: a
    /// backend that gave up on a hung camera starts over here. An error while a picture is being
    /// taken (the list is not touched then) or when the backend is unusable.
    fn list(&self) -> Result<Vec<CameraInfo>, AppError>;
    /// Makes sure the camera is found and open, so the next `arm` and `trigger` can work. A clear
    /// error in words when there is none, or when something else holds it.
    fn connect(&self) -> Result<CameraInfo, AppError>;
}
