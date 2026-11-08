//! The camera's live view (H-14): one JPEG frame at a time, so the capture view can frame and focus
//! the film (the lens is on manual) and judge the exposure before a triplet is taken. The libgphoto2
//! calls are in `gphoto/session.rs`; the worker that leaves live view before every capture is in
//! `gphoto/worker.rs`.

use gtp_core::AppError;

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
}
