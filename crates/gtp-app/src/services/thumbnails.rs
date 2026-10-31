//! Thumbnails that follow the recipe (L-09). A thumbnail is a rebuildable cache of the frame as its
//! saved recipe renders it: after `recipe_save` or `recipe_apply_to` the affected frames are
//! queued here and re-rendered in the background, and a missing thumbnail file is rebuilt when it
//! is asked for.
//!
//! The commands never wait for a render. Requests for the same frame within a quiet period (see
//! [`DEBOUNCE`]) become one render of the newest saved recipe, and a batch (a whole roll) is worked
//! through one frame at a time. A thumbnail is rendered from the cached proxy when the frame is
//! open in Develop, otherwise from the original, decoded for the purpose and not cached, so a
//! batch cannot push the frame being edited out of the cache. A failure keeps the old thumbnail
//! and is logged; it never interrupts anything.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use gtp_core::recipe::OutputSpace;
use gtp_core::{AppError, ErrorCode, FrameId};
use gtp_io::encode_jpeg;

use crate::jobs::LibraryEvents;
use crate::library_fs::thumb_rel;
use crate::pipeline::{analyze, render, RenderTarget};
use crate::services::develop::DevelopService;
use crate::services::import::{THUMB_EDGE, THUMB_QUALITY};
use crate::services::library::Library;
use crate::services::roll_frame;

/// Requests that arrive within this time of each other are collapsed into one batch.
pub const DEBOUNCE: Duration = Duration::from_millis(500);
/// A batch starts after this long even if requests keep arriving.
const MAX_WAIT: Duration = Duration::from_secs(5);

/// What the worker thread and the on-demand path share.
struct Shared {
    /// One render at a time, so a rebuild asked for twice is done once.
    turn: Mutex<()>,
    renders: AtomicUsize,
}

/// A request to regenerate the thumbnails of some frames of a library.
struct Request {
    library: Arc<Library>,
    develop: Arc<DevelopService>,
    events: Arc<dyn LibraryEvents>,
    frames: Vec<FrameId>,
}

pub struct ThumbnailService {
    shared: Arc<Shared>,
    debounce: Duration,
    /// Started by the first request.
    sender: Mutex<Option<Sender<Request>>>,
}

impl Default for ThumbnailService {
    fn default() -> Self {
        Self::new(DEBOUNCE)
    }
}

impl ThumbnailService {
    pub fn new(debounce: Duration) -> Self {
        Self {
            shared: Arc::new(Shared {
                turn: Mutex::new(()),
                renders: AtomicUsize::new(0),
            }),
            debounce,
            sender: Mutex::new(None),
        }
    }

    /// Queues the frames' thumbnails for regeneration and returns at once. When they are done,
    /// `events` hears of the changed frames (once per batch).
    pub fn request(
        &self,
        library: Arc<Library>,
        develop: Arc<DevelopService>,
        events: Arc<dyn LibraryEvents>,
        frames: Vec<FrameId>,
    ) {
        if frames.is_empty() {
            return;
        }
        let mut slot = self.sender.lock().unwrap_or_else(PoisonError::into_inner);
        let request = Request {
            library,
            develop,
            events,
            frames,
        };
        let request = match slot.as_ref() {
            Some(sender) => match sender.send(request) {
                Ok(()) => return,
                Err(mpsc::SendError(request)) => request,
            },
            None => request,
        };
        // No worker yet (or it ended): start one with this request.
        let (tx, rx) = mpsc::channel();
        let shared = Arc::clone(&self.shared);
        let debounce = self.debounce;
        let started = std::thread::Builder::new()
            .name("thumbnails".into())
            .spawn(move || work(&rx, &shared, debounce));
        match started {
            Ok(_) if tx.send(request).is_ok() => *slot = Some(tx),
            Ok(_) => tracing::warn!("the thumbnail worker ended at once"),
            Err(err) => tracing::warn!(%err, "the thumbnail worker could not start"),
        }
    }

    /// How many thumbnails were rendered (tests).
    pub fn renders(&self) -> usize {
        self.shared.renders.load(Ordering::SeqCst)
    }

    /// The frame's thumbnail as bytes. A missing file is rebuilt from the frame's saved recipe
    /// first (the auto recipe when it has none); `NotFound` remains when the frame, or its
    /// original, is gone. A read-only library is not written to, so nothing is rebuilt there.
    pub fn thumbnail(
        &self,
        develop: &DevelopService,
        library: &Library,
        frame: FrameId,
    ) -> Result<Vec<u8>, AppError> {
        let missing = match roll_frame::frame_thumbnail(library, frame) {
            Err(err) if err.code == ErrorCode::NotFound && !library.is_read_only() => err,
            other => return other,
        };
        let _turn = self
            .shared
            .turn
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Another request may have rebuilt it while this one waited for its turn.
        if let Ok(bytes) = roll_frame::frame_thumbnail(library, frame) {
            return Ok(bytes);
        }
        if !self.regenerate(develop, library, frame)? {
            return Err(missing);
        }
        roll_frame::frame_thumbnail(library, frame)
    }

    /// Renders and stores one thumbnail. False when the frame no longer exists (it was deleted
    /// while queued), so nothing was written. The caller holds the turn.
    fn regenerate(
        &self,
        develop: &DevelopService,
        library: &Library,
        frame: FrameId,
    ) -> Result<bool, AppError> {
        match library.read(|c| c.get_frame(frame)) {
            Ok(_) => {}
            Err(err) if err.code == ErrorCode::NotFound => return Ok(false),
            Err(err) => return Err(err),
        }
        let input = develop.thumbnail_input(library, frame)?;
        let recipe = match library.read(|c| c.load_recipe(frame))? {
            Some(recipe) => recipe,
            None => analyze(&input)?.auto_recipe,
        };
        let image = render(
            &input,
            &recipe,
            RenderTarget::Preview {
                max_edge: THUMB_EDGE,
            },
        )?;
        encode_jpeg(
            &image,
            OutputSpace::Srgb,
            THUMB_QUALITY,
            &library.resolve(&thumb_rel(frame))?,
        )?;
        self.shared.renders.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }
}

/// The frames of one library waiting to be regenerated.
struct Batch {
    library: Arc<Library>,
    develop: Arc<DevelopService>,
    events: Arc<dyn LibraryEvents>,
    frames: Vec<FrameId>,
}

fn add(batches: &mut Vec<Batch>, request: Request) {
    let at = batches
        .iter()
        .position(|b| Arc::ptr_eq(&b.library, &request.library));
    let batch = match at {
        Some(at) => &mut batches[at],
        None => {
            batches.push(Batch {
                library: request.library,
                develop: request.develop,
                events: request.events,
                frames: Vec::new(),
            });
            let last = batches.len() - 1;
            &mut batches[last]
        }
    };
    for frame in request.frames {
        if !batch.frames.contains(&frame) {
            batch.frames.push(frame);
        }
    }
}

/// The worker: waits for a request, collects more until the quiet period passes, then works
/// through the batch. Ends when the service is dropped.
fn work(requests: &Receiver<Request>, shared: &Arc<Shared>, debounce: Duration) {
    // A throwaway service over the shared state, so the on-demand and background paths use the
    // same render code and the same turn.
    let service = ThumbnailService {
        shared: Arc::clone(shared),
        debounce,
        sender: Mutex::new(None),
    };
    while let Ok(first) = requests.recv() {
        let mut batches = Vec::new();
        add(&mut batches, first);
        let started = Instant::now();
        while started.elapsed() < MAX_WAIT {
            match requests.recv_timeout(debounce) {
                Ok(next) => add(&mut batches, next),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
        for batch in batches {
            let mut done = Vec::new();
            for frame in &batch.frames {
                let _turn = shared.turn.lock().unwrap_or_else(PoisonError::into_inner);
                match service.regenerate(&batch.develop, &batch.library, *frame) {
                    Ok(true) => done.push(*frame),
                    Ok(false) => tracing::debug!(%frame, "frame gone, thumbnail skipped"),
                    Err(err) => {
                        tracing::warn!(%frame, code = ?err.code, message = %err.message, "thumbnail not regenerated");
                    }
                }
            }
            if !done.is_empty() {
                batch.events.frames_changed(&done);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use gtp_core::{Recipe, RollId};

    use super::*;
    use crate::services::develop::save_recipe;
    use crate::services::testkit::Fixture;

    /// Collects what `LibraryEvents` hears, so a test can wait for a batch.
    struct Heard(Mutex<mpsc::Sender<Vec<FrameId>>>);

    impl LibraryEvents for Heard {
        fn library_changed(&self, _roll: Option<RollId>) {}

        fn frames_changed(&self, frames: &[FrameId]) {
            let _ = self
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(frames.to_vec());
        }
    }

    const QUICK: Duration = Duration::from_millis(60);
    const WAIT: Duration = Duration::from_secs(20);

    fn listener() -> (Arc<Heard>, mpsc::Receiver<Vec<FrameId>>) {
        let (tx, rx) = mpsc::channel();
        (Arc::new(Heard(Mutex::new(tx))), rx)
    }

    impl Fixture {
        fn thumb(&self, frame: FrameId) -> Option<Vec<u8>> {
            std::fs::read(self.library.resolve(&thumb_rel(frame)).unwrap()).ok()
        }

        fn ask(&self, svc: &ThumbnailService, events: &Arc<Heard>, frames: &[FrameId]) {
            svc.request(
                Arc::clone(&self.library),
                Arc::clone(&self.service),
                Arc::clone(events) as Arc<dyn LibraryEvents>,
                frames.to_vec(),
            );
        }
    }

    /// The JPEG with the creation time of its embedded ICC profile (bytes 24 to 36 of the
    /// profile header) zeroed: two encodes a second apart differ only there.
    fn without_profile_time(mut jpeg: Vec<u8>) -> Vec<u8> {
        let marker = b"ICC_PROFILE\0";
        let at = jpeg
            .windows(marker.len())
            .position(|w| w == marker)
            .expect("a thumbnail embeds its colour profile");
        // The marker is followed by the chunk number and count, then the profile itself.
        let start = at + marker.len() + 2;
        jpeg[start + 24..start + 36].fill(0);
        jpeg
    }

    fn brighter() -> Recipe {
        let mut r = Recipe::default();
        r.tone.exposure_ev = 1.0;
        r
    }

    #[test]
    fn a_thumbnail_is_rebuilt_from_the_saved_recipe_and_the_change_is_announced() {
        let t = Fixture::new("thumb-recipe");
        let id = t.frame(1, 640, 480);
        let svc = ThumbnailService::new(QUICK);
        let (events, heard) = listener();

        save_recipe(&t.library, id, Recipe::default()).unwrap();
        t.ask(&svc, &events, &[id]);
        assert_eq!(heard.recv_timeout(WAIT).unwrap(), [id]);
        let before = t.thumb(id).unwrap();
        assert!(before.starts_with(&[0xFF, 0xD8]), "a JPEG");

        save_recipe(&t.library, id, brighter()).unwrap();
        t.ask(&svc, &events, &[id]);
        assert_eq!(heard.recv_timeout(WAIT).unwrap(), [id]);
        let after = t.thumb(id).unwrap();
        assert_ne!(before, after, "the picture changed with the recipe");

        // It is exactly what that recipe renders: same bytes as rendering it directly.
        let input = t.service.thumbnail_input(&t.library, id).unwrap();
        let direct = render(
            &input,
            &brighter(),
            RenderTarget::Preview {
                max_edge: THUMB_EDGE,
            },
        )
        .unwrap();
        let path = t.dir.join("direct.jpg");
        encode_jpeg(&direct, OutputSpace::Srgb, THUMB_QUALITY, &path).unwrap();
        assert_eq!(
            without_profile_time(std::fs::read(path).unwrap()),
            without_profile_time(after)
        );
        assert_eq!(svc.renders(), 2);
    }

    #[test]
    fn requests_never_wait_and_repeats_within_the_quiet_period_make_one_render() {
        let t = Fixture::new("thumb-debounce");
        let id = t.frame(1, 640, 480);
        // Long enough that the test sees the request return first.
        let svc = ThumbnailService::new(Duration::from_millis(400));
        let (events, heard) = listener();
        save_recipe(&t.library, id, brighter()).unwrap();

        let asked = Instant::now();
        for _ in 0..6 {
            t.ask(&svc, &events, &[id]);
        }
        assert!(
            asked.elapsed() < Duration::from_millis(300),
            "six requests took {:?}",
            asked.elapsed()
        );
        assert_eq!(svc.renders(), 0, "nothing rendered yet");

        assert_eq!(heard.recv_timeout(WAIT).unwrap(), [id]);
        assert_eq!(svc.renders(), 1, "one render for six requests");
        assert!(
            heard.recv_timeout(Duration::from_millis(800)).is_err(),
            "and one announcement"
        );
    }

    #[test]
    fn a_batch_is_announced_once_with_every_frame_that_succeeded() {
        let t = Fixture::new("thumb-batch");
        let (a, b, c) = (
            t.frame(1, 400, 300),
            t.frame(2, 400, 300),
            t.frame(3, 400, 300),
        );
        // A quiet period long enough that a slow machine cannot split the two requests.
        let svc = ThumbnailService::new(Duration::from_millis(400));
        let (events, heard) = listener();
        for id in [a, b, c] {
            save_recipe(&t.library, id, brighter()).unwrap();
        }
        // A frame that was deleted while queued is skipped and writes nothing.
        let gone = FrameId::new();
        t.ask(&svc, &events, &[a, b]);
        t.ask(&svc, &events, &[b, c, gone]);

        let mut heard_frames = heard.recv_timeout(WAIT).unwrap();
        heard_frames.sort();
        let mut want = vec![a, b, c];
        want.sort();
        assert_eq!(heard_frames, want);
        assert!(heard.recv_timeout(Duration::from_millis(300)).is_err());
        assert_eq!(svc.renders(), 3);
        assert!(t.thumb(gone).is_none());
    }

    #[test]
    fn a_failure_keeps_the_old_thumbnail_and_announces_nothing() {
        let t = Fixture::new("thumb-fail");
        let id = t.frame(1, 400, 300);
        let svc = ThumbnailService::new(QUICK);
        let (events, heard) = listener();
        t.ask(&svc, &events, &[id]);
        heard.recv_timeout(WAIT).unwrap();
        let old = t.thumb(id).unwrap();

        // The original goes missing, then the recipe changes.
        let original = t
            .library
            .read(|c| c.sources_of(id))
            .unwrap()
            .remove(0)
            .rel_path;
        std::fs::remove_file(t.library.resolve(std::path::Path::new(&original)).unwrap()).unwrap();
        save_recipe(&t.library, id, brighter()).unwrap();
        t.ask(&svc, &events, &[id]);

        assert!(heard.recv_timeout(Duration::from_millis(800)).is_err());
        assert_eq!(t.thumb(id).unwrap(), old);
        assert_eq!(svc.renders(), 1);
    }

    #[test]
    fn regenerating_does_not_push_the_open_frame_out_of_the_cache() {
        let t = Fixture::new("thumb-cache");
        let open = t.frame(1, 400, 300);
        let others: Vec<FrameId> = (2..=7).map(|n| t.frame(n, 400, 300)).collect();
        t.service.open(&t.library, open).unwrap();
        let svc = ThumbnailService::new(QUICK);
        let (events, heard) = listener();

        t.ask(&svc, &events, &others);
        assert_eq!(heard.recv_timeout(WAIT).unwrap().len(), others.len());
        assert_eq!(
            t.service.cached(),
            [open],
            "the open frame stays, no others join"
        );
    }

    #[test]
    fn a_missing_thumbnail_is_rebuilt_when_asked_for() {
        let t = Fixture::new("thumb-missing");
        let id = t.frame(1, 400, 300);
        let svc = ThumbnailService::default();
        // No thumbnail was ever made (the frame has no stored recipe: the auto recipe is used).
        assert!(t.thumb(id).is_none());
        let bytes = svc.thumbnail(&t.service, &t.library, id).unwrap();
        assert!(bytes.starts_with(&[0xFF, 0xD8]));
        assert_eq!(t.thumb(id).unwrap(), bytes, "and stored for next time");
        assert_eq!(svc.renders(), 1);

        // Asked again, it is read, not rendered.
        svc.thumbnail(&t.service, &t.library, id).unwrap();
        assert_eq!(svc.renders(), 1);

        // Deleted by hand: rebuilt from the saved recipe.
        save_recipe(&t.library, id, brighter()).unwrap();
        std::fs::remove_file(t.library.resolve(&thumb_rel(id)).unwrap()).unwrap();
        let rebuilt = svc.thumbnail(&t.service, &t.library, id).unwrap();
        assert_ne!(rebuilt, bytes);
    }

    #[test]
    fn a_read_only_library_is_not_written_to() {
        use std::io::{Seek, SeekFrom, Write};

        use crate::services::roll_frame::{roll_create, RollInput};
        use crate::services::testkit::add_frame;

        let dir = std::env::temp_dir().join(format!(
            "gtp-thumb-readonly-{}-{}",
            std::process::id(),
            FrameId::new()
        ));
        let library = Library::create(&dir, "Test").unwrap();
        let roll = roll_create(
            &library,
            RollInput {
                name: "R".into(),
                ..RollInput::default()
            },
        )
        .unwrap()
        .id;
        let id = add_frame(&library, roll, 1, 400, 300);
        drop(library);
        // The library was made by a newer app version: its schema number is higher than ours.
        let db = dir.join(crate::library_fs::DB_FILE);
        {
            let mut file = std::fs::OpenOptions::new().write(true).open(&db).unwrap();
            file.seek(SeekFrom::Start(60)).unwrap();
            file.write_all(&(gtp_catalog::SCHEMA_VERSION + 1).to_be_bytes())
                .unwrap();
        }

        let readonly = Library::open(&dir).unwrap();
        assert!(readonly.is_read_only());
        let svc = ThumbnailService::default();
        let err = svc
            .thumbnail(&DevelopService::default(), &readonly, id)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound, "nothing rebuilt");
        assert!(!readonly.resolve(&thumb_rel(id)).unwrap().exists());
        assert_eq!(svc.renders(), 0);
        drop(readonly);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_can_be_rebuilt_for_a_frame_or_original_that_is_gone() {
        let t = Fixture::new("thumb-gone");
        let svc = ThumbnailService::default();
        let code = |r: Result<Vec<u8>, AppError>| r.unwrap_err().code;
        assert_eq!(
            code(svc.thumbnail(&t.service, &t.library, FrameId::new())),
            ErrorCode::NotFound
        );
        let bare = t.bare(1);
        assert_eq!(
            code(svc.thumbnail(&t.service, &t.library, bare)),
            ErrorCode::NotFound,
            "a frame with no original"
        );
        assert!(t.thumb(bare).is_none());
        assert_eq!(svc.renders(), 0);
    }
}
