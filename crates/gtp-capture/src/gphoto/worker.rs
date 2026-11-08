//! The worker that owns the camera, and [`GphotoCamera`], the [`CameraSource`] that talks to it.
//!
//! One thread owns the [`Session`] (so nothing from libgphoto2 ever crosses a thread) and handles
//! requests one at a time. `trigger` sends a capture request and returns; `wait_for_image` waits
//! on that request's own reply channel with the caller's timeout. A call into libgphoto2 cannot be
//! interrupted, so when a reply does not come in time the camera is marked **unusable**: every
//! later call fails at once with words until [`CameraFinder::list`] starts a new worker (the
//! "refresh" gesture) or the app restarts. The stuck thread is left alone, never joined; when its
//! call finally returns, its result goes to a channel nobody reads any more and is dropped, so it
//! can never be mistaken for the next picture. Dropping the camera never waits for the thread.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use gtp_core::{AppError, ErrorCode};

use crate::camera::{CameraFinder, CameraInfo, CameraSource, CapturedFile};
use crate::preview::CameraPreview;
use crate::settings::{
    check_change, clean_value, CameraSetting, CameraSettingKind, CameraSettings,
};

/// The text libgphoto2 gives for a USB claim failure (`GP_ERROR_IO_USB_CLAIM`: "Could not claim the
/// USB device"), which the app turns into words (ADR 0004 amendment 7).
pub const CLAIM_MARKER: &str = "claim the usb device";

/// What a camera session does. Lives on the worker thread only, so it needs no `Send`; the factory
/// that builds it runs on that thread.
pub trait Session {
    /// The cameras libgphoto2 sees now (without opening any).
    fn list(&mut self) -> Result<Vec<CameraInfo>, SessionError>;
    /// Finds the camera and opens it, claiming its USB interface. Called only when none is open.
    fn open(&mut self) -> Result<CameraInfo, SessionError>;
    /// Takes one picture, downloads it and removes it from the camera's memory.
    fn capture(&mut self) -> Result<CapturedFile, SessionError>;
    /// Reads the three exposure settings, in `CameraSettingKind::ALL` order. A setting the camera
    /// does not have (unknown key, not supported, not a menu) is `CameraSetting::absent`, not an
    /// error; a failure of the link is an error. Called with the camera open.
    fn settings(&mut self) -> Result<Vec<CameraSetting>, SessionError>;
    /// Writes one setting. The worker has already checked that `value` is one of the choices of a
    /// writable setting and reads the settings again afterwards.
    fn set_setting(&mut self, kind: CameraSettingKind, value: &str) -> Result<(), SessionError>;
    /// Reads one live-view frame as JPEG, putting the camera in live view if it needs that.
    /// Called with the camera open.
    fn preview_frame(&mut self) -> Result<Vec<u8>, SessionError>;
    /// Leaves live view (a camera that has no live-view switch is left as it is). The worker calls
    /// it only after a frame was read, before a capture and on a stop.
    fn leave_live_view(&mut self) -> Result<(), SessionError>;
    /// Lets go of the camera (after a lost link, or when the worker ends).
    fn close(&mut self);
}

/// What kind of failure a session reports; each kind has its own words for the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionErrorKind {
    /// macOS (or another program) holds the USB device.
    Claim,
    /// No camera is plugged in, or it is off.
    NoCamera,
    /// The camera is busy with something else.
    Busy,
    /// The link broke (cable, power, a USB error). The session is closed and reopened next time.
    Lost,
    /// Anything else.
    Other,
}

/// A failure of the session, with libgphoto2's own text for the details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionError {
    /// The kind, which picks the words.
    pub kind: SessionErrorKind,
    /// The technical text (goes into `details`, never into the message).
    pub message: String,
}

impl SessionError {
    /// A failure of `kind` with the library's text.
    pub fn new(kind: SessionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The kind after looking at the text too: a claim failure is recognised by its message even
    /// when the binding filed it under another code.
    fn effective_kind(&self) -> SessionErrorKind {
        if self.message.to_ascii_lowercase().contains(CLAIM_MARKER) {
            SessionErrorKind::Claim
        } else {
            self.kind
        }
    }

    /// The error the user sees, in words, with the library's text as details.
    pub fn into_app_error(self) -> AppError {
        let (code, words) = match self.effective_kind() {
            SessionErrorKind::Claim => (
                ErrorCode::Io,
                "macOS is holding the camera, so the app cannot use it. Log out any other user accounts on this Mac, quit Photos and Image Capture, then unplug the camera, plug it in again and try once more.",
            ),
            SessionErrorKind::NoCamera => (
                ErrorCode::NotFound,
                "No camera was found. Connect the camera with its USB cable, switch it on and try again.",
            ),
            SessionErrorKind::Busy => (
                ErrorCode::Io,
                "The camera is busy. Wait a moment and try again.",
            ),
            SessionErrorKind::Lost => (
                ErrorCode::Io,
                "The connection to the camera was lost. Check its USB cable and that it is switched on, then try again.",
            ),
            SessionErrorKind::Other => (
                ErrorCode::Io,
                "The camera reported an error. Check that it is switched on and not in use by another program, then try again.",
            ),
        };
        AppError::new(code, words).with_details(self.message)
    }
}

/// How long the worker gets for the calls that are not a picture. A picture's time is the
/// `timeout` of `wait_for_image`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Finding and opening the camera (a claim failure comes back in well under a second).
    pub open: Duration,
    /// Listing the cameras.
    pub list: Duration,
    /// Reading or changing the exposure settings.
    pub settings: Duration,
    /// Reading one live-view frame, or leaving live view.
    pub preview: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            open: Duration::from_secs(20),
            list: Duration::from_secs(10),
            settings: Duration::from_secs(10),
            preview: Duration::from_secs(5),
        }
    }
}

fn unusable_error() -> AppError {
    AppError::new(
        ErrorCode::Io,
        "The camera stopped answering earlier and is switched off in the app to keep the light safe. Switch the camera off and on, unplug and replug its USB cable, then refresh the camera list. If that does not help, restart the app.",
    )
}

fn timed_out_error() -> AppError {
    AppError::new(
        ErrorCode::Io,
        "The camera did not answer in time, so the app stopped using it. Switch the camera off and on, unplug and replug its USB cable, then refresh the camera list. If that does not help, restart the app.",
    )
}

fn busy_error() -> AppError {
    AppError::new(
        ErrorCode::Io,
        "A picture is being taken. Wait for it to finish, then try again.",
    )
}

type Reply<T> = Sender<Result<T, SessionError>>;

enum Request {
    List(Reply<Vec<CameraInfo>>),
    Open(Reply<CameraInfo>),
    Capture(Reply<CapturedFile>),
    Settings(Reply<Vec<CameraSetting>>),
    SetSetting(CameraSettingKind, String, Reply<Vec<CameraSetting>>),
    Preview(Reply<Vec<u8>>),
    PreviewStop(Reply<()>),
}

/// What the camera object and its worker both see; one per worker thread.
#[derive(Default)]
struct LinkState {
    /// Set when a reply did not come in time (or the worker is gone). Never cleared: a new worker
    /// has a new state.
    unusable: AtomicBool,
    /// The camera is open and the last call on it worked.
    connected: AtomicBool,
    /// Capture requests sent and not yet finished by the worker.
    in_flight: AtomicUsize,
}

impl LinkState {
    fn unusable(&self) -> bool {
        self.unusable.load(Ordering::SeqCst)
    }

    fn mark_unusable(&self) {
        self.unusable.store(true, Ordering::SeqCst);
        self.connected.store(false, Ordering::SeqCst);
    }
}

/// The way to one worker thread.
struct Link {
    requests: Sender<Request>,
    state: Arc<LinkState>,
}

/// Builds the session on the worker thread.
type Factory = dyn Fn() -> Result<Box<dyn Session>, SessionError> + Send + Sync;

/// What the camera object and the finders share: the way to build sessions and the current link.
struct Hub {
    factory: Arc<Factory>,
    timeouts: Timeouts,
    link: Mutex<Arc<Link>>,
}

impl Hub {
    fn start(factory: Arc<Factory>, timeouts: Timeouts) -> Result<Arc<Self>, AppError> {
        let link = spawn_worker(&factory)?;
        Ok(Arc::new(Self {
            factory,
            timeouts,
            link: Mutex::new(link),
        }))
    }

    fn link(&self) -> Arc<Link> {
        Arc::clone(&self.link.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Sends the request `make` builds to the worker and waits up to `timeout` for the reply; a
    /// reply that does not come makes the camera unusable.
    fn ask<T>(
        &self,
        make: impl FnOnce(Reply<T>) -> Request,
        timeout: Duration,
    ) -> Result<T, AppError> {
        let link = self.link();
        if link.state.unusable() {
            return Err(unusable_error());
        }
        let (reply, answer) = channel();
        if link.requests.send(make(reply)).is_err() {
            link.state.mark_unusable();
            return Err(unusable_error());
        }
        match answer.recv_timeout(timeout) {
            Ok(result) => result.map_err(SessionError::into_app_error),
            Err(RecvTimeoutError::Timeout) => {
                tracing::error!("the camera worker did not answer within {timeout:?}");
                link.state.mark_unusable();
                Err(timed_out_error())
            }
            Err(RecvTimeoutError::Disconnected) => {
                tracing::error!("the camera worker ended without answering");
                link.state.mark_unusable();
                Err(unusable_error())
            }
        }
    }

    fn refuse_while_busy(&self) -> Result<(), AppError> {
        let link = self.link();
        if link.state.unusable() {
            return Err(unusable_error());
        }
        if link.state.in_flight.load(Ordering::SeqCst) > 0 {
            Err(busy_error())
        } else {
            Ok(())
        }
    }

    fn connect(&self) -> Result<CameraInfo, AppError> {
        self.refuse_while_busy()?;
        self.ask(Request::Open, self.timeouts.open)
    }

    fn settings(&self) -> Result<Vec<CameraSetting>, AppError> {
        self.refuse_while_busy()?;
        self.ask(Request::Settings, self.timeouts.settings)
    }

    /// Reads the settings, checks that `value` is a choice of a writable setting, then writes it
    /// and returns the settings read again. Nothing is written when the check fails.
    fn set_setting(
        &self,
        kind: CameraSettingKind,
        value: &str,
    ) -> Result<Vec<CameraSetting>, AppError> {
        let value = clean_value(value)?;
        let current = self.settings()?;
        check_change(&current, kind, &value)?;
        self.ask(
            |reply| Request::SetSetting(kind, value, reply),
            self.timeouts.settings,
        )
    }

    /// One live-view frame. Refused while a picture is being taken.
    fn preview_frame(&self) -> Result<Vec<u8>, AppError> {
        self.refuse_while_busy()?;
        self.ask(Request::Preview, self.timeouts.preview)
    }

    /// Leaves live view (nothing happens when the camera is not in it).
    fn preview_stop(&self) -> Result<(), AppError> {
        self.refuse_while_busy()?;
        self.ask(Request::PreviewStop, self.timeouts.preview)
    }

    /// Drops the current worker (it ends when its call returns, if it is stuck in one) and starts
    /// a new one. A new worker's first call still waits behind a call libgphoto2 never returned
    /// from, since the binding runs every call on one thread; it then times out and the app needs
    /// a restart.
    fn reconnect(&self) -> Result<(), AppError> {
        let fresh = spawn_worker(&self.factory)?;
        let old = {
            let mut link = self.link.lock().unwrap_or_else(PoisonError::into_inner);
            std::mem::replace(&mut *link, fresh)
        };
        old.state.mark_unusable();
        Ok(())
    }

    fn list(&self) -> Result<Vec<CameraInfo>, AppError> {
        if self.link().state.unusable() {
            // Refreshing is the way back from an unusable camera.
            self.reconnect()?;
        }
        self.refuse_while_busy()?;
        self.ask(Request::List, self.timeouts.list)
    }
}

fn spawn_worker(factory: &Arc<Factory>) -> Result<Arc<Link>, AppError> {
    let (requests, incoming) = channel();
    let state = Arc::new(LinkState::default());
    let worker_state = Arc::clone(&state);
    let factory = Arc::clone(factory);
    std::thread::Builder::new()
        .name("gphoto-camera".to_owned())
        .spawn(move || run(&incoming, &worker_state, factory.as_ref()))
        .map_err(|err| AppError::from_io(&err))?;
    Ok(Arc::new(Link { requests, state }))
}

type SessionSlot = Result<Box<dyn Session>, SessionError>;

/// The worker: builds the session, then serves requests until the camera object lets go of its
/// channel or the link was declared unusable.
fn run(incoming: &Receiver<Request>, state: &LinkState, factory: &Factory) {
    let mut session: SessionSlot = factory();
    let mut opened: Option<CameraInfo> = None;
    // True from the first live-view frame until live view is left; the camera may be in live view
    // while it is set.
    let mut live_view = false;
    while let Ok(request) = incoming.recv() {
        if state.unusable() {
            // Abandoned while it was stuck: whatever it was asked since is moot.
            break;
        }
        match request {
            Request::List(reply) => deliver(&reply, with(&mut session, |s| s.list())),
            Request::Open(reply) => {
                let result = ensure_open(&mut session, &mut opened, state);
                deliver(&reply, result);
            }
            Request::Capture(reply) => {
                let result = ensure_open(&mut session, &mut opened, state).and_then(|_| {
                    if live_view {
                        // A capture straight after live view needs it left first. If leaving
                        // fails the capture is still tried: its own error says what is wrong.
                        if let Err(err) = with(&mut session, |s| s.leave_live_view()) {
                            tracing::warn!("could not leave live view before a capture: {err:?}");
                        }
                        live_view = false;
                    }
                    with(&mut session, |s| s.capture())
                });
                if drop_if_lost(&result, &mut session, &mut opened, state) {
                    live_view = false;
                }
                state.in_flight.fetch_sub(1, Ordering::SeqCst);
                deliver(&reply, result);
            }
            Request::Settings(reply) => {
                let result = ensure_open(&mut session, &mut opened, state)
                    .and_then(|_| with(&mut session, |s| s.settings()));
                let _ = drop_if_lost(&result, &mut session, &mut opened, state);
                deliver(&reply, result);
            }
            Request::Preview(reply) => {
                let result = ensure_open(&mut session, &mut opened, state).and_then(|_| {
                    live_view = true;
                    with(&mut session, |s| s.preview_frame())
                });
                if drop_if_lost(&result, &mut session, &mut opened, state) {
                    live_view = false;
                }
                deliver(&reply, result);
            }
            Request::PreviewStop(reply) => {
                let result = if live_view {
                    let left = ensure_open(&mut session, &mut opened, state)
                        .and_then(|_| with(&mut session, |s| s.leave_live_view()));
                    if left.is_ok() {
                        live_view = false;
                    }
                    left
                } else {
                    Ok(())
                };
                if drop_if_lost(&result, &mut session, &mut opened, state) {
                    live_view = false;
                }
                deliver(&reply, result);
            }
            Request::SetSetting(kind, value, reply) => {
                let result = ensure_open(&mut session, &mut opened, state).and_then(|_| {
                    with(&mut session, |s| {
                        s.set_setting(kind, &value)?;
                        s.settings()
                    })
                });
                let _ = drop_if_lost(&result, &mut session, &mut opened, state);
                deliver(&reply, result);
            }
        }
    }
    close(&mut session);
}

/// After a failure that says the link is gone, closes the session so the next request opens it
/// again; returns whether it did (a new session is not in live view).
fn drop_if_lost<T>(
    result: &Result<T, SessionError>,
    session: &mut SessionSlot,
    opened: &mut Option<CameraInfo>,
    state: &LinkState,
) -> bool {
    if let Err(err) = result {
        if matches!(
            err.effective_kind(),
            SessionErrorKind::Lost | SessionErrorKind::NoCamera
        ) {
            close(session);
            *opened = None;
            state.connected.store(false, Ordering::SeqCst);
            return true;
        }
    }
    false
}

fn close(session: &mut SessionSlot) {
    if let Ok(s) = session {
        s.close();
    }
}

/// Runs `call` on the session, or fails with the error the factory gave.
fn with<T>(
    session: &mut SessionSlot,
    call: impl FnOnce(&mut dyn Session) -> Result<T, SessionError>,
) -> Result<T, SessionError> {
    match session {
        Ok(s) => call(s.as_mut()),
        Err(err) => Err(err.clone()),
    }
}

fn ensure_open(
    session: &mut SessionSlot,
    opened: &mut Option<CameraInfo>,
    state: &LinkState,
) -> Result<CameraInfo, SessionError> {
    if let Some(info) = opened {
        return Ok(info.clone());
    }
    let info = with(session, |s| s.open())?;
    *opened = Some(info.clone());
    state.connected.store(true, Ordering::SeqCst);
    Ok(info)
}

fn deliver<T>(reply: &Reply<T>, result: Result<T, SessionError>) {
    if reply.send(result).is_err() {
        // The caller gave up (a timeout): a late result is dropped here and never used.
        tracing::warn!("a late camera result was discarded");
    }
}

/// The camera as the capture sequence sees it: a [`CameraSource`] backed by the worker thread.
/// `Send`, whatever the binding's types are, because none of them leaves the worker.
pub struct GphotoCamera {
    hub: Arc<Hub>,
    /// The reply of the capture `trigger` started, until `wait_for_image` collects it.
    pending: Option<Receiver<Result<CapturedFile, SessionError>>>,
}

impl GphotoCamera {
    /// A camera over `factory`'s sessions, with the default timeouts. The factory runs on the worker
    /// thread. Nothing is opened yet: the camera is found when it is first needed.
    pub fn with_session(
        factory: impl Fn() -> Result<Box<dyn Session>, SessionError> + Send + Sync + 'static,
    ) -> Result<Self, AppError> {
        Self::with_timeouts(factory, Timeouts::default())
    }

    /// [`GphotoCamera::with_session`] with other timeouts (tests shorten them).
    pub fn with_timeouts(
        factory: impl Fn() -> Result<Box<dyn Session>, SessionError> + Send + Sync + 'static,
        timeouts: Timeouts,
    ) -> Result<Self, AppError> {
        Ok(Self {
            hub: Hub::start(Arc::new(factory), timeouts)?,
            pending: None,
        })
    }

    /// A handle for finding and listing cameras that stays usable while a job holds the camera.
    pub fn finder(&self) -> GphotoFinder {
        GphotoFinder {
            hub: Arc::clone(&self.hub),
        }
    }

    /// Finds and opens the camera now (see [`CameraFinder::connect`]).
    pub fn connect(&self) -> Result<CameraInfo, AppError> {
        self.hub.connect()
    }

    /// Drops the worker and starts a new one, clearing the unusable mark.
    pub fn reconnect(&self) -> Result<(), AppError> {
        self.hub.reconnect()
    }

    /// True when a call timed out and the camera is out of use until a reconnect or restart.
    pub fn is_unusable(&self) -> bool {
        self.hub.link().state.unusable()
    }

    fn discard_stale(&mut self) {
        if self.pending.take().is_some() {
            // The picture of an earlier, abandoned shot: its result is dropped.
            tracing::warn!("an earlier picture was never collected; it is discarded");
        }
    }
}

impl CameraSource for GphotoCamera {
    fn is_connected(&self) -> bool {
        // Atomics only: never waits for the worker, which may be inside a blocked call.
        let link = self.hub.link();
        link.state.connected.load(Ordering::SeqCst) && !link.state.unusable()
    }

    fn arm(&mut self) -> Result<(), AppError> {
        self.discard_stale();
        self.hub
            .ask(Request::Open, self.hub.timeouts.open)
            .map(|_| ())
    }

    fn trigger(&mut self) -> Result<(), AppError> {
        self.discard_stale();
        let link = self.hub.link();
        if link.state.unusable() {
            return Err(unusable_error());
        }
        let (reply, answer) = channel();
        link.state.in_flight.fetch_add(1, Ordering::SeqCst);
        if link.requests.send(Request::Capture(reply)).is_err() {
            link.state.in_flight.fetch_sub(1, Ordering::SeqCst);
            link.state.mark_unusable();
            return Err(unusable_error());
        }
        self.pending = Some(answer);
        Ok(())
    }

    fn wait_for_image(&mut self, timeout: Duration) -> Result<CapturedFile, AppError> {
        let Some(answer) = self.pending.take() else {
            return Err(AppError::internal(
                "wait_for_image was called before trigger",
            ));
        };
        let link = self.hub.link();
        if link.state.unusable() {
            return Err(unusable_error());
        }
        match answer.recv_timeout(timeout) {
            Ok(result) => result.map_err(SessionError::into_app_error),
            Err(RecvTimeoutError::Timeout) => {
                // `answer` is dropped on return: the late picture, if one ever comes, is discarded.
                tracing::error!("the camera gave no picture within {timeout:?}");
                link.state.mark_unusable();
                Err(timed_out_error())
            }
            Err(RecvTimeoutError::Disconnected) => {
                tracing::error!("the camera worker ended without a picture");
                link.state.mark_unusable();
                Err(unusable_error())
            }
        }
    }
}

/// Lists and connects the camera of a [`GphotoCamera`] without holding it, so the UI can ask while
/// a job has the camera.
#[derive(Clone)]
pub struct GphotoFinder {
    hub: Arc<Hub>,
}

impl CameraFinder for GphotoFinder {
    fn list(&self) -> Result<Vec<CameraInfo>, AppError> {
        self.hub.list()
    }

    fn connect(&self) -> Result<CameraInfo, AppError> {
        self.hub.connect()
    }
}

impl CameraSettings for GphotoFinder {
    fn settings(&self) -> Result<Vec<CameraSetting>, AppError> {
        self.hub.settings()
    }

    fn set_setting(
        &self,
        kind: CameraSettingKind,
        value: &str,
    ) -> Result<Vec<CameraSetting>, AppError> {
        self.hub.set_setting(kind, value)
    }
}

impl CameraPreview for GphotoFinder {
    fn preview_frame(&self) -> Result<Vec<u8>, AppError> {
        self.hub.preview_frame()
    }

    fn preview_stop(&self) -> Result<(), AppError> {
        self.hub.preview_stop()
    }
}
