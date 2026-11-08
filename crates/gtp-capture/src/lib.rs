//! Guided RGB capture: Scanlight V4 serial driver, camera backends (libgphoto2, watched folder), per-frame R/G/B sequence.
//!
//! This holds the light side (the [`LightSource`] port, the [`ScanlightV4`] serial driver and the
//! recording [`FakeLight`]), the [`CameraSource`] port with its [`FakeCamera`], and the
//! [`CaptureSequence`] that takes one R, G, B triplet with the hardware-safety rules, and the
//! libgphoto2 backend ([`GphotoCamera`]: a worker over a small session trait that builds everywhere;
//! the real session needs the `tether` feature on macOS). The watched-folder backend comes with H-10.

#![forbid(unsafe_code)]

pub mod calibration;
pub mod camera;
pub mod fakes;
pub mod gphoto;
pub mod light;
pub mod preview;
pub mod profile;
pub mod scanlight;
pub mod sequence;
pub mod settings;

pub use calibration::{calibrate, ChannelMeter, Region};
pub use camera::{CameraFinder, CameraInfo, CameraSource, CapturedFile};
pub use fakes::{
    CameraCall, FakeCamera, FakeCameraHandle, FakeLight, FakeLightHandle, LightCommand, Shot,
};
pub use gphoto::{configure_library_paths, GphotoCamera, GphotoFinder};
pub use light::{LightInfo, LightSource, LightStatus, PortInfo};
pub use preview::CameraPreview;
pub use profile::{
    CameraBackend, CaptureProfile, CaptureProfilePatch, MAX_SETTLE_MS, VBUS_WARN_MV,
};
pub use scanlight::{list_ports, ScanlightV4};
pub use sequence::{
    CaptureOutcome, CaptureSequence, CaptureWarning, LightGuard, Pause, ThreadPause,
    CAPTURE_TIMEOUT,
};
pub use settings::{CameraSetting, CameraSettingKind, CameraSettings};
