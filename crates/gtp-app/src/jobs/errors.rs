//! The last errors of finished jobs, kept in memory for the diagnostics export (specification
//! section 12). A job that was cancelled on request is not an error and is not recorded.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use gtp_core::{AppError, ErrorCode};
use serde::Serialize;

use super::JobKind;

/// How many errors are kept; the oldest is dropped first.
pub const MAX_JOB_ERRORS: usize = 20;

/// One job error as the diagnostics zip shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobErrorEntry {
    pub at_unix_ms: u64,
    pub job: String,
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}

/// The ring of recent job errors. Cheap to share behind an `Arc`.
#[derive(Default)]
pub struct JobErrorLog(Mutex<VecDeque<JobErrorEntry>>);

impl JobErrorLog {
    pub(super) fn record(&self, kind: JobKind, error: &AppError) {
        if error.code == ErrorCode::Cancelled {
            return;
        }
        let at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let mut entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.len() == MAX_JOB_ERRORS {
            entries.pop_front();
        }
        entries.push_back(JobErrorEntry {
            at_unix_ms,
            job: format!("{kind:?}"),
            code: format!("{:?}", error.code),
            message: error.message.clone(),
            details: error.details.clone(),
        });
    }

    /// The recorded errors, oldest first.
    pub fn recent(&self) -> Vec<JobErrorEntry> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_errors_and_skips_cancellations() {
        let log = JobErrorLog::default();
        log.record(
            JobKind::Export,
            &AppError::new(ErrorCode::Cancelled, "The job was cancelled."),
        );
        assert!(log.recent().is_empty());

        for i in 0..MAX_JOB_ERRORS + 5 {
            log.record(
                JobKind::Import,
                &AppError::new(ErrorCode::Io, format!("failure {i}")).with_details("detail"),
            );
        }
        let recent = log.recent();
        assert_eq!(recent.len(), MAX_JOB_ERRORS);
        assert_eq!(recent[0].message, "failure 5");
        assert_eq!(recent[MAX_JOB_ERRORS - 1].message, "failure 24");
        assert_eq!(recent[0].job, "Import");
        assert_eq!(recent[0].code, "Io");
        assert_eq!(recent[0].details.as_deref(), Some("detail"));
    }
}
