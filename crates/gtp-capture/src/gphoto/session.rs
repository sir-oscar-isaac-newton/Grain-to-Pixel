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
use gphoto2::widget::{RadioWidget, TextWidget, ToggleWidget};
use gphoto2::{Camera, Context};

use crate::camera::{CameraInfo, CapturedFile};
use crate::gphoto::worker::{Session, SessionError, SessionErrorKind};
use crate::settings::{is_missing_setting, CameraSetting, CameraSettingKind};

/// A NEF of the D600 is about 26 MB; anything this big is not a picture from a camera.
const MAX_PICTURE_BYTES: u64 = 512 * 1024 * 1024;

/// A live-view frame of the D600 is about 7 KB; anything this big is not a preview.
const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
/// The libgphoto2 configuration key that switches live view on Nikon bodies.
const VIEWFINDER_KEY: &str = "viewfinder";
/// The keys of the live-view zoom (H-18), moving the zoomed area, the image size and the reason
/// live view cannot start, on Nikon bodies (tested on a D600).
const ZOOM_KEY: &str = "liveviewimagezoomratio";
const AREA_KEY: &str = "changeafarea";
const IMAGE_SIZE_KEY: &str = "imagesize";
const PROHIBIT_KEY: &str = "liveviewprohibit";

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
        let camera = open_camera(self.camera.as_ref())?;
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

    fn settings(&mut self) -> Result<Vec<CameraSetting>, SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        CameraSettingKind::ALL
            .iter()
            .map(|kind| read_setting(camera, *kind))
            .collect()
    }

    fn set_setting(&mut self, kind: CameraSettingKind, value: &str) -> Result<(), SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        // Read the widget again so that the change is made on what the camera has now.
        let Some(widget) = fetch_widget(camera, kind)? else {
            return Err(SessionError::new(
                SessionErrorKind::Other,
                format!("the camera has no \"{}\" setting", kind.key()),
            ));
        };
        widget.set_choice(value).map_err(|err| map_error(&err))?;
        camera
            .set_config(&widget)
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))
    }

    fn preview_frame(&mut self) -> Result<Vec<u8>, SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        let frame = camera
            .capture_preview()
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))?;
        let bytes = frame
            .get_data(&self.context)
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))?;
        if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
            return Err(SessionError::new(
                SessionErrorKind::Other,
                format!(
                    "the live-view frame has an unusable size ({} bytes)",
                    bytes.len()
                ),
            ));
        }
        Ok(bytes.into_vec())
    }

    fn leave_live_view(&mut self) -> Result<(), SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        // Nikon bodies leave live view when `viewfinder` is set to 0. A camera without that widget
        // (or with another kind of it) has nothing to leave.
        let fetched = camera
            .config_key::<ToggleWidget>(VIEWFINDER_KEY)
            .try_wait()
            .map_err(|err| channel_error(&err))?;
        let widget = match fetched {
            Ok(widget) => widget,
            Err(err) => {
                let unsupported = matches!(
                    err.kind(),
                    ErrorKind::BadParameters | ErrorKind::NotSupported
                );
                return if unsupported || err.to_string().contains("Expected ToggleWidget") {
                    Ok(())
                } else {
                    Err(map_error(&err))
                };
            }
        };
        widget.set_toggled(false);
        camera
            .set_config(&widget)
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))
    }

    fn preview_zoom_choices(&mut self) -> Result<Vec<String>, SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        Ok(match fetch_radio(camera, ZOOM_KEY)? {
            Some(widget) => widget.choices_iter().collect(),
            None => Vec::new(),
        })
    }

    fn set_preview_zoom(&mut self, value: &str) -> Result<(), SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        let Some(widget) = fetch_radio(camera, ZOOM_KEY)? else {
            return Err(missing_key(ZOOM_KEY));
        };
        widget.set_choice(value).map_err(|err| map_error(&err))?;
        camera
            .set_config(&widget)
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))
    }

    fn image_size(&mut self) -> Result<Option<String>, SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        Ok(fetch_radio(camera, IMAGE_SIZE_KEY)?.map(|widget| widget.choice()))
    }

    fn set_preview_area(&mut self, area: &str) -> Result<(), SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        let Some(widget) = fetch_text(camera, AREA_KEY)? else {
            return Err(missing_key(AREA_KEY));
        };
        widget.set_value(area).map_err(|err| map_error(&err))?;
        camera
            .set_config(&widget)
            .try_wait()
            .map_err(|err| channel_error(&err))?
            .map_err(|err| map_error(&err))
    }

    fn preview_prohibit(&mut self) -> Result<Option<String>, SessionError> {
        let camera = open_camera(self.camera.as_ref())?;
        Ok(fetch_text(camera, PROHIBIT_KEY)?.map(|widget| widget.value()))
    }

    fn close(&mut self) {
        self.camera = None;
    }
}

fn missing_key(key: &str) -> SessionError {
    SessionError::new(
        SessionErrorKind::Other,
        format!("the camera has no \"{key}\" setting"),
    )
}

/// True when failing to read a key means the camera does not have it as that kind of widget.
fn is_missing_key(err: &gphoto2::Error, expected: &str) -> bool {
    matches!(
        err.kind(),
        ErrorKind::BadParameters | ErrorKind::NotSupported
    ) || err.to_string().contains(expected)
}

/// The menu widget for `key`, or `None` when the camera lacks it or has it as another kind.
fn fetch_radio(camera: &Camera, key: &str) -> Result<Option<RadioWidget>, SessionError> {
    let fetched = camera
        .config_key::<RadioWidget>(key)
        .try_wait()
        .map_err(|err| channel_error(&err))?;
    match fetched {
        Ok(widget) => Ok(Some(widget)),
        Err(err) if is_missing_key(&err, "Expected RadioWidget") => Ok(None),
        Err(err) => Err(map_error(&err)),
    }
}

/// The text widget for `key`, or `None` when the camera lacks it or has it as another kind.
fn fetch_text(camera: &Camera, key: &str) -> Result<Option<TextWidget>, SessionError> {
    let fetched = camera
        .config_key::<TextWidget>(key)
        .try_wait()
        .map_err(|err| channel_error(&err))?;
    match fetched {
        Ok(widget) => Ok(Some(widget)),
        Err(err) if is_missing_key(&err, "Expected TextWidget") => Ok(None),
        Err(err) => Err(map_error(&err)),
    }
}

fn open_camera(camera: Option<&Camera>) -> Result<&Camera, SessionError> {
    camera.ok_or_else(|| SessionError::new(SessionErrorKind::NoCamera, "no camera is open"))
}

/// The menu widget of `kind`, or `None` when the camera does not have that setting (see
/// [`is_missing_setting`]); a failure of the link is an error.
fn fetch_widget(
    camera: &Camera,
    kind: CameraSettingKind,
) -> Result<Option<RadioWidget>, SessionError> {
    let fetched = camera
        .config_key::<RadioWidget>(kind.key())
        .try_wait()
        .map_err(|err| channel_error(&err))?;
    match fetched {
        Ok(widget) => Ok(Some(widget)),
        Err(err) => {
            let unsupported = matches!(
                err.kind(),
                ErrorKind::BadParameters | ErrorKind::NotSupported
            );
            if is_missing_setting(unsupported, &err.to_string()) {
                Ok(None)
            } else {
                Err(map_error(&err))
            }
        }
    }
}

fn read_setting(camera: &Camera, kind: CameraSettingKind) -> Result<CameraSetting, SessionError> {
    Ok(match fetch_widget(camera, kind)? {
        Some(widget) => CameraSetting::from_camera(
            kind,
            &widget.choice(),
            widget.choices_iter(),
            !widget.readonly(),
        ),
        None => CameraSetting::absent(kind),
    })
}

/// The factory [`crate::GphotoCamera::new`] gives the worker.
pub fn open_session() -> Result<Box<dyn Session>, SessionError> {
    Ok(Box::new(GphotoSession::new()?))
}
