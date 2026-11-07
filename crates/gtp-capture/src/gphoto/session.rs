//! The real [`Session`], over the `gphoto2` crate (3.4.1, LGPL-2.1, compiled into the app). Only compiled with the
//! `tether` feature on macOS, and kept as thin as it can be: every decision is in `worker.rs`.
//! It uses what the R-03 probe (`tools/spikes/gphoto-probe`) was verified with on the author's Mac,
//! with `try_wait` in place of `wait` so that no call can panic on a closed channel.
//!
//! The binding runs every libgphoto2 call on one global thread of its own and the calling thread
//! blocks until the call returns; the worker thread is that calling thread. Neither a `Context`
//! nor a `Camera` leaves this thread.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gphoto2::error::ErrorKind;
use gphoto2::{Camera, Context};

use crate::camera::{CameraInfo, CapturedFile};
use crate::gphoto::worker::{Session, SessionError, SessionErrorKind};

/// A NEF of the D600 is about 26 MB; anything this big is not a picture from a camera.
const MAX_PICTURE_BYTES: u64 = 512 * 1024 * 1024;

/// The camera of the session: the binding's context, and the camera once opened.
pub struct GphotoSession {
    context: Context,
    camera: Option<Camera>,
}

impl GphotoSession {
    /// A session with a new libgphoto2 context. Call it on the worker thread.
    pub fn new() -> Result<Self, SessionError> {
        Ok(Self {
            context: Context::new().map_err(|err| map_error(&err))?,
            camera: None,
        })
    }
}

fn map_error(err: &gphoto2::Error) -> SessionError {
    let kind = match err.kind() {
        ErrorKind::IoUsbClaim => SessionErrorKind::Claim,
        ErrorKind::ModelNotFound | ErrorKind::IoUsbFind | ErrorKind::UnknownPort => {
            SessionErrorKind::NoCamera
        }
        ErrorKind::CameraBusy => SessionErrorKind::Busy,
        ErrorKind::Io
        | ErrorKind::IoRead
        | ErrorKind::IoWrite
        | ErrorKind::IoUsbClearHalt
        | ErrorKind::IoLock
        | ErrorKind::Timeout => SessionErrorKind::Lost,
        _ => SessionErrorKind::Other,
    };
    SessionError::new(kind, err.to_string())
}

/// The binding's channel closing means its thread is gone.
fn channel_error(err: &impl std::fmt::Display) -> SessionError {
    SessionError::new(
        SessionErrorKind::Lost,
        format!("the libgphoto2 thread stopped: {err}"),
    )
}

fn info_for(model: &str) -> CameraInfo {
    CameraInfo {
        id: format!("gphoto:{model}"),
        name: model.to_owned(),
    }
}

/// A folder of its own for one download, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, SessionError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gtp-gphoto-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).map_err(io_error)?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(err) = fs::remove_dir_all(&self.0) {
            tracing::debug!("removing the download folder failed: {err}");
        }
    }
}

fn io_error(err: std::io::Error) -> SessionError {
    SessionError::new(SessionErrorKind::Other, format!("disk: {err}"))
}

/// A file name that is safe to create: the camera's own name (`DSC_0001.NEF`) minus anything odd.
fn file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    if clean.is_empty() {
        "picture.nef".to_owned()
    } else {
        clean
    }
}

impl Session for GphotoSession {
    fn list(&mut self) -> Result<Vec<CameraInfo>, SessionError> {
        let found = self
            .context
            .list_cameras()
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))?;
        let mut cameras: Vec<CameraInfo> = Vec::new();
        for camera in found {
            // The D600 is listed once per USB port entry (R-03): one camera, one row.
            let info = info_for(&camera.model);
            if !cameras.contains(&info) {
                cameras.push(info);
            }
        }
        Ok(cameras)
    }

    fn open(&mut self) -> Result<CameraInfo, SessionError> {
        let camera = self
            .context
            .autodetect_camera()
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))?;
        let info = info_for(&camera.abilities().model());
        self.camera = Some(camera);
        Ok(info)
    }

    fn capture(&mut self) -> Result<CapturedFile, SessionError> {
        let Some(camera) = self.camera.as_ref() else {
            return Err(SessionError::new(
                SessionErrorKind::NoCamera,
                "no camera is open",
            ));
        };
        let remote = camera
            .capture_image()
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))?;
        let folder = remote.folder().to_string();
        let name = remote.name().to_string();

        let scratch = Scratch::new()?;
        let target = scratch.path().join(file_name(&name));
        camera
            .fs()
            .download_to(&folder, &name, &target)
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))?;
        let size = fs::metadata(&target).map_err(io_error)?.len();
        if size == 0 || size > MAX_PICTURE_BYTES {
            return Err(SessionError::new(
                SessionErrorKind::Other,
                format!("the downloaded picture has an unusable size ({size} bytes)"),
            ));
        }
        let bytes = fs::read(&target).map_err(io_error)?;

        // The picture is ours now; a failure to delete it from the camera is only worth a warning.
        match camera.fs().delete_file(&folder, &name).try_wait() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!("could not delete the picture from the camera: {err}"),
            Err(err) => tracing::warn!("could not delete the picture from the camera: {err}"),
        }
        Ok(CapturedFile { name, bytes })
    }

    fn close(&mut self) {
        self.camera = None;
    }
}

/// The factory [`crate::GphotoCamera::new`] gives the worker.
pub fn open_session() -> Result<Box<dyn Session>, SessionError> {
    Ok(Box::new(GphotoSession::new()?))
}
