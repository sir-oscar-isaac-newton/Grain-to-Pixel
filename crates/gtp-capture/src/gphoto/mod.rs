//! The libgphoto2 camera backend (H-09, ADR 0004 amendments 2, 7 and 8).
//!
//! libgphoto2's capture call blocks and cannot be interrupted, so a dedicated worker thread owns the
//! camera and its session, and [`GphotoCamera`] only talks to it through channels. What is
//! libgphoto2-specific is one small trait ([`Session`]); the worker, the timeouts, the error words
//! and the library-path logic are plain Rust that builds and is tested on every platform. The real
//! session (`session.rs`) exists only with the `tether` feature on macOS.

mod paths;
#[cfg(all(feature = "tether", target_os = "macos"))]
mod session;
mod worker;

#[cfg(test)]
mod tests;

pub use paths::{bundled_library_dirs, configure_library_paths, LibraryDirs};
pub use worker::{
    GphotoCamera, GphotoFinder, Session, SessionError, SessionErrorKind, Timeouts, CLAIM_MARKER,
};

#[cfg(all(feature = "tether", target_os = "macos"))]
impl GphotoCamera {
    /// The camera over libgphoto2. Starts the worker thread but opens nothing: the camera is found
    /// when it is first needed, so one plugged in after the app started works. Needs the driver
    /// paths set first when running from an app bundle ([`configure_library_paths`]).
    pub fn new() -> Result<Self, gtp_core::AppError> {
        Self::with_session(session::open_session)
    }
}
