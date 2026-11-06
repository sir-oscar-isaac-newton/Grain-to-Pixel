//! Guided RGB capture: Scanlight V4 serial driver, camera backends (libgphoto2, watched folder), per-frame R/G/B sequence.
//!
//! Today this holds the light side: the [`LightSource`] port, the [`ScanlightV4`] serial driver
//! and the recording [`FakeLight`]. The camera backends and the sequence come with H-08.

#![forbid(unsafe_code)]

pub mod fakes;
pub mod light;
pub mod scanlight;

pub use fakes::{FakeLight, FakeLightHandle, LightCommand};
pub use light::{LightInfo, LightSource, LightStatus};
pub use scanlight::ScanlightV4;
