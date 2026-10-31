//! Cooperative cancellation: the manager raises a flag, the job polls it and stops itself.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A shared "please stop" flag. Raising it twice, or after the job ended, is harmless.
#[derive(Debug, Clone, Default)]
pub(crate) struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_raised_flag_is_seen_by_every_clone() {
        let flag = CancelFlag::default();
        let seen_by_job = flag.clone();
        assert!(!seen_by_job.is_cancelled());
        flag.cancel();
        flag.cancel();
        assert!(seen_by_job.is_cancelled());
    }
}
