//! Background jobs (docs/specification.md section 3): long work such as import, scan or export
//! runs on its own thread, reports progress, can be cancelled, and ends with exactly one `done` or
//! `error`, delivered to a [`JobSink`] (the desktop shell turns these into `job://*` events).
//!
//! Jobs run in parallel, one thread each. Where only one job may use a device at a time (the
//! scanner, the camera), the service that starts those jobs enforces it.

mod cancel;
mod manager;

use std::sync::{Arc, Mutex, PoisonError};

use gtp_core::{AppError, ErrorCode, JobId, JobResult};

pub use manager::JobManager;

pub(crate) use cancel::CancelFlag;

/// What a job does; used for logs and for listing running jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JobKind {
    Import,
    Scan,
    Export,
    Capture,
    LibraryVerify,
}

/// Receives the events of every job. Called from job threads, so it must be `Send + Sync`.
///
/// For one job the calls arrive in order: any number of `progress`, then exactly one `done` or
/// `error`, and nothing after it.
pub trait JobSink: Send + Sync {
    fn progress(&self, id: JobId, stage: &str, fraction: f32);
    fn done(&self, id: JobId, result: JobResult);
    fn error(&self, id: JobId, error: AppError);
}

/// A progress report is sent only when the stage changes, the fraction moves by at least this
/// much, or the fraction reaches 1, so a tight loop cannot flood the UI.
pub const PROGRESS_STEP: f32 = 0.01;

/// Event state of one job, shared by its context and the manager; the lock keeps the order.
struct Emitter {
    id: JobId,
    sink: Arc<dyn JobSink>,
    ended: bool,
    last: Option<(String, f32)>,
}

impl Emitter {
    fn progress(&mut self, stage: &str, fraction: f32) {
        if self.ended {
            return;
        }
        let fraction = if fraction.is_nan() {
            0.0
        } else {
            fraction.clamp(0.0, 1.0)
        };
        let due = match &self.last {
            None => true,
            Some((last_stage, last_fraction)) => {
                last_stage != stage
                    || (fraction - last_fraction).abs() >= PROGRESS_STEP
                    || (fraction == 1.0 && *last_fraction < 1.0)
            }
        };
        if due {
            self.last = Some((stage.to_owned(), fraction));
            self.sink.progress(self.id, stage, fraction);
        }
    }

    fn finish(&mut self, outcome: Result<JobResult, AppError>) {
        if self.ended {
            return;
        }
        self.ended = true;
        match outcome {
            Ok(result) => self.sink.done(self.id, result),
            Err(error) => self.sink.error(self.id, error),
        }
    }
}

/// What a running job sees: its id, a way to report progress and its cancel flag.
pub struct JobCtx {
    id: JobId,
    cancel: CancelFlag,
    emitter: Arc<Mutex<Emitter>>,
}

impl JobCtx {
    pub fn id(&self) -> JobId {
        self.id
    }

    /// Reports progress through `stage` (a short label such as "decoding"); `fraction` runs
    /// from 0 to 1 and is clamped. May be called from any thread of the job.
    pub fn progress(&self, stage: &str, fraction: f32) {
        self.emitter
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .progress(stage, fraction);
    }

    /// True once the job was asked to stop.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// `Err(Cancelled)` once the job was asked to stop, for use with `?` between steps.
    pub fn check_cancelled(&self) -> Result<(), AppError> {
        if self.is_cancelled() {
            Err(AppError::new(
                ErrorCode::Cancelled,
                "The job was cancelled.",
            ))
        } else {
            Ok(())
        }
    }

    /// A context outside any manager, for testing job bodies; returns its cancel flag.
    #[cfg(test)]
    pub(crate) fn standalone() -> (Self, CancelFlag) {
        struct Quiet;
        impl JobSink for Quiet {
            fn progress(&self, _: JobId, _: &str, _: f32) {}
            fn done(&self, _: JobId, _: JobResult) {}
            fn error(&self, _: JobId, _: AppError) {}
        }
        let id = JobId::new();
        let cancel = CancelFlag::default();
        let ctx = Self {
            id,
            cancel: cancel.clone(),
            emitter: Arc::new(Mutex::new(Emitter {
                id,
                sink: Arc::new(Quiet),
                ended: false,
                last: None,
            })),
        };
        (ctx, cancel)
    }
}

/// Library change notifications (the `library://changed` event), so the UI refreshes while a job
/// adds or removes frames.
pub trait LibraryEvents: Send + Sync {
    /// Frames of `roll` (or of several rolls when `None`) were added, changed or removed.
    fn library_changed(&self, roll: Option<gtp_core::RollId>);
}
