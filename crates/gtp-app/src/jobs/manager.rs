//! Starts jobs on their own threads, tracks the running ones and cancels them on request.

use std::any::Any;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError};

use gtp_core::{AppError, JobId, JobResult};

use super::{CancelFlag, Emitter, JobCtx, JobErrorEntry, JobErrorLog, JobKind, JobSink};

struct Running {
    kind: JobKind,
    cancel: CancelFlag,
}

type RunningJobs = Arc<Mutex<HashMap<JobId, Running>>>;

/// Runs jobs and delivers their events to one [`JobSink`]. Cheap to share behind an `Arc`.
pub struct JobManager {
    sink: Arc<dyn JobSink>,
    running: RunningJobs,
    errors: Arc<JobErrorLog>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned())
}

impl JobManager {
    pub fn new(sink: Arc<dyn JobSink>) -> Self {
        Self {
            sink,
            running: Arc::default(),
            errors: Arc::default(),
        }
    }

    /// Starts `work` on a new thread and returns its id at once. `work` reports through the
    /// [`JobCtx`]; its return value becomes the job's single `done` or `error` event. A panic
    /// inside `work` becomes an `Internal` error instead of taking the app down.
    pub fn spawn<F>(&self, kind: JobKind, work: F) -> Result<JobId, AppError>
    where
        F: FnOnce(&JobCtx) -> Result<JobResult, AppError> + Send + 'static,
    {
        let id = JobId::new();
        let cancel = CancelFlag::default();
        let ctx = JobCtx {
            id,
            cancel: cancel.clone(),
            emitter: Arc::new(Mutex::new(Emitter {
                id,
                kind,
                sink: Arc::clone(&self.sink),
                errors: Arc::clone(&self.errors),
                ended: false,
                last: None,
            })),
        };
        // Registered before the thread starts, so a cancel right after `spawn` is never lost.
        lock(&self.running).insert(id, Running { kind, cancel });

        let running = Arc::clone(&self.running);
        let started = std::thread::Builder::new()
            .name(format!("job-{kind:?}-{id}"))
            .spawn(move || {
                let outcome =
                    catch_unwind(AssertUnwindSafe(|| work(&ctx))).unwrap_or_else(|payload| {
                        Err(AppError::internal(format!(
                            "job panicked: {}",
                            panic_message(payload.as_ref())
                        )))
                    });
                // Unregistered first: once the UI has the final event, cancelling finds nothing.
                lock(&running).remove(&id);
                lock(&ctx.emitter).finish(outcome);
            });
        if let Err(err) = started {
            lock(&self.running).remove(&id);
            return Err(AppError::from_io(&err));
        }
        Ok(id)
    }

    /// Asks a job to stop. True when the job was still running; cancelling a finished or unknown
    /// job does nothing.
    pub fn cancel(&self, id: JobId) -> bool {
        match lock(&self.running).get(&id) {
            Some(job) => {
                job.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Asks every running job to stop (on quit).
    pub fn cancel_all(&self) {
        for job in lock(&self.running).values() {
            job.cancel.cancel();
        }
    }

    /// The errors of the last finished jobs, oldest first (cancellations left out).
    pub fn recent_errors(&self) -> Vec<JobErrorEntry> {
        self.errors.recent()
    }

    /// The running jobs and their kinds.
    pub fn running(&self) -> Vec<(JobId, JobKind)> {
        lock(&self.running)
            .iter()
            .map(|(id, job)| (*id, job.kind))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::time::Duration;

    use gtp_core::ErrorCode;

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);
    const QUIET: Duration = Duration::from_millis(100);

    #[derive(Debug, Clone, PartialEq)]
    enum Event {
        Progress(JobId, String, f32),
        Done(JobId, JobResult),
        Error(JobId, AppError),
    }

    /// Forwards every event to a channel, in the order the sink receives them.
    struct FakeSink(Mutex<Sender<Event>>);

    impl JobSink for FakeSink {
        fn progress(&self, id: JobId, stage: &str, fraction: f32) {
            let _ = lock(&self.0).send(Event::Progress(id, stage.to_owned(), fraction));
        }
        fn done(&self, id: JobId, result: JobResult) {
            let _ = lock(&self.0).send(Event::Done(id, result));
        }
        fn error(&self, id: JobId, error: AppError) {
            let _ = lock(&self.0).send(Event::Error(id, error));
        }
    }

    fn manager() -> (JobManager, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        (JobManager::new(Arc::new(FakeSink(Mutex::new(tx)))), rx)
    }

    /// Every event of one job up to and including its terminal event; then checks that nothing
    /// else arrives.
    fn events_until_end(rx: &Receiver<Event>) -> Vec<Event> {
        let mut events = Vec::new();
        loop {
            let event = rx.recv_timeout(WAIT).unwrap();
            let end = !matches!(event, Event::Progress(..));
            events.push(event);
            if end {
                break;
            }
        }
        assert_eq!(rx.recv_timeout(QUIET), Err(RecvTimeoutError::Timeout));
        events
    }

    #[test]
    fn progress_arrives_in_order_and_done_exactly_once() {
        let (jobs, rx) = manager();
        let id = jobs
            .spawn(JobKind::Export, |ctx| {
                for i in 0..=50 {
                    ctx.progress("encoding", i as f32 / 50.0);
                }
                Ok(JobResult::Finished)
            })
            .unwrap();
        let events = events_until_end(&rx);
        let fractions: Vec<f32> = events
            .iter()
            .filter_map(|e| match e {
                Event::Progress(i, stage, f) if *i == id && stage == "encoding" => Some(*f),
                _ => None,
            })
            .collect();
        assert_eq!(fractions.len(), 51);
        assert!(fractions.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(events.last(), Some(&Event::Done(id, JobResult::Finished)));
        assert!(jobs.running().is_empty());
    }

    #[test]
    fn a_returned_error_is_the_single_terminal_event() {
        let (jobs, rx) = manager();
        let id = jobs
            .spawn(JobKind::Import, |_| {
                Err(AppError::new(ErrorCode::UnsupportedFormat, "not a TIFF"))
            })
            .unwrap();
        let events = events_until_end(&rx);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], Event::Error(i, e) if *i == id && e.code == ErrorCode::UnsupportedFormat)
        );
    }

    #[test]
    fn a_panicking_job_ends_with_an_internal_error() {
        let (jobs, rx) = manager();
        #[allow(clippy::panic)]
        let id = jobs
            .spawn(JobKind::Scan, |ctx| {
                ctx.progress("scanning", 0.5);
                panic!("scanner driver blew up");
            })
            .unwrap();
        let events = events_until_end(&rx);
        assert_eq!(events.len(), 2);
        match &events[1] {
            Event::Error(i, e) => {
                assert_eq!(*i, id);
                assert_eq!(e.code, ErrorCode::Internal);
                assert!(e.details.as_deref().unwrap_or("").contains("blew up"));
            }
            other => unreachable!("{other:?}"),
        }
    }

    #[test]
    fn cancel_stops_a_running_job_cooperatively() {
        let (jobs, rx) = manager();
        let (started_tx, started_rx) = mpsc::channel();
        let id = jobs
            .spawn(JobKind::Scan, move |ctx| {
                let _ = started_tx.send(());
                loop {
                    ctx.check_cancelled()?;
                    std::thread::sleep(Duration::from_millis(5));
                }
            })
            .unwrap();
        started_rx.recv_timeout(WAIT).unwrap();
        assert_eq!(jobs.running(), vec![(id, JobKind::Scan)]);
        assert!(jobs.cancel(id));
        let events = events_until_end(&rx);
        assert!(
            matches!(&events[..], [Event::Error(i, e)] if *i == id && e.code == ErrorCode::Cancelled)
        );
    }

    #[test]
    fn failed_jobs_are_remembered_for_diagnostics_but_cancelled_ones_are_not() {
        let (jobs, rx) = manager();
        jobs.spawn(JobKind::Import, |_| {
            Err(AppError::new(ErrorCode::UnsupportedFormat, "not a TIFF"))
        })
        .unwrap();
        events_until_end(&rx);
        jobs.spawn(JobKind::Export, |ctx| {
            ctx.progress("encoding", 0.5);
            Ok(JobResult::Finished)
        })
        .unwrap();
        events_until_end(&rx);
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let id = jobs
            .spawn(JobKind::Scan, move |ctx| {
                let _ = go_rx.recv_timeout(WAIT);
                ctx.check_cancelled().map(|()| JobResult::Finished)
            })
            .unwrap();
        assert!(jobs.cancel(id));
        go_tx.send(()).unwrap();
        events_until_end(&rx);

        let recent = jobs.recent_errors();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].job, "Import");
        assert_eq!(recent[0].code, "UnsupportedFormat");
        assert_eq!(recent[0].message, "not a TIFF");
    }

    #[test]
    fn cancel_after_the_end_or_of_an_unknown_job_is_harmless() {
        let (jobs, rx) = manager();
        let id = jobs
            .spawn(JobKind::Export, |_| Ok(JobResult::Finished))
            .unwrap();
        events_until_end(&rx);
        assert!(!jobs.cancel(id));
        assert!(!jobs.cancel(JobId::new()));
        jobs.cancel_all();
        assert_eq!(rx.recv_timeout(QUIET), Err(RecvTimeoutError::Timeout));
    }

    #[test]
    fn a_job_that_finishes_anyway_after_cancel_reports_done() {
        let (jobs, rx) = manager();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let id = jobs
            .spawn(JobKind::Export, move |_| {
                let _ = go_rx.recv_timeout(WAIT);
                Ok(JobResult::Finished)
            })
            .unwrap();
        assert!(jobs.cancel(id));
        go_tx.send(()).unwrap();
        assert_eq!(
            events_until_end(&rx),
            vec![Event::Done(id, JobResult::Finished)]
        );
    }

    #[test]
    fn progress_is_throttled_but_stage_changes_and_completion_get_through() {
        let (jobs, rx) = manager();
        jobs.spawn(JobKind::Import, |ctx| {
            for i in 0..=10_000 {
                ctx.progress("hashing", i as f32 / 10_000.0);
            }
            ctx.progress("thumbnails", 0.0);
            ctx.progress("thumbnails", 0.004);
            ctx.progress("thumbnails", f32::NAN);
            ctx.progress("thumbnails", 7.0);
            Ok(JobResult::Finished)
        })
        .unwrap();
        let events = events_until_end(&rx);
        let progress: Vec<(String, f32)> = events
            .iter()
            .filter_map(|e| match e {
                Event::Progress(_, s, f) => Some((s.clone(), *f)),
                _ => None,
            })
            .collect();
        let hashing = progress.iter().filter(|(s, _)| s == "hashing").count();
        assert!((90..=102).contains(&hashing), "{hashing} hashing events");
        assert!(progress.contains(&("hashing".to_owned(), 1.0)));
        let thumbs: Vec<f32> = progress
            .iter()
            .filter(|(s, _)| s == "thumbnails")
            .map(|(_, f)| *f)
            .collect();
        assert_eq!(
            thumbs,
            vec![0.0, 1.0],
            "small steps and NaN dropped, 7 clamped"
        );
    }

    #[test]
    fn jobs_run_in_parallel() {
        let (jobs, rx) = manager();
        let (tx, partner_rx) = mpsc::channel::<()>();
        // The first job can only finish if the second runs at the same time.
        let waiting = jobs
            .spawn(JobKind::Scan, move |_| {
                partner_rx.recv_timeout(WAIT).map_err(|_| {
                    AppError::internal("the second job never ran alongside the first")
                })?;
                Ok(JobResult::Finished)
            })
            .unwrap();
        let signalling = jobs
            .spawn(JobKind::Export, move |_| {
                let _ = tx.send(());
                Ok(JobResult::Finished)
            })
            .unwrap();
        let mut done = vec![
            rx.recv_timeout(WAIT).unwrap(),
            rx.recv_timeout(WAIT).unwrap(),
        ];
        done.sort_by_key(|e| matches!(e, Event::Done(i, _) if *i == waiting));
        assert_eq!(
            done,
            vec![
                Event::Done(signalling, JobResult::Finished),
                Event::Done(waiting, JobResult::Finished)
            ]
        );
    }

    #[test]
    fn cancel_all_stops_every_running_job() {
        let (jobs, rx) = manager();
        let (started_tx, started_rx) = mpsc::channel();
        for _ in 0..3 {
            let started = started_tx.clone();
            jobs.spawn(JobKind::Import, move |ctx| {
                let _ = started.send(());
                while !ctx.is_cancelled() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                ctx.check_cancelled().map(|()| JobResult::Finished)
            })
            .unwrap();
        }
        for _ in 0..3 {
            started_rx.recv_timeout(WAIT).unwrap();
        }
        jobs.cancel_all();
        for _ in 0..3 {
            let event = rx.recv_timeout(WAIT).unwrap();
            assert!(matches!(event, Event::Error(_, e) if e.code == ErrorCode::Cancelled));
        }
    }
}
