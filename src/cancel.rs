//! Stopping work that's under way: a job the user cancels, a chat whose
//! page went away. Whoever does the work holds a [`Cancel`] and looks at it
//! between steps; a `claude` it runs is stopped the moment it's cancelled
//! ([`crate::claude`]); whoever wants the work stopped holds a clone and
//! calls [`Cancel::cancel`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// How often a wait looks at whether it's been cancelled.
const LOOK: Duration = Duration::from_millis(50);

/// Whether the work it's given to has been asked to stop, shared by every
/// clone.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Cancel {
        Cancel::default()
    }

    /// Asks the work to stop. It can't be taken back.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Waits `time`, or less if it's cancelled meanwhile: `false` then.
    pub fn sleep(&self, time: Duration) -> bool {
        let until = Instant::now() + time;
        loop {
            if self.is_cancelled() {
                return false;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            thread::sleep(left.min(LOOK));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_clone_sees_it_cancelled() {
        let cancel = Cancel::new();
        let other = cancel.clone();
        assert!(!other.is_cancelled());
        cancel.cancel();
        assert!(other.is_cancelled());
    }

    #[test]
    fn a_wait_ends_early_once_cancelled() {
        let cancel = Cancel::new();
        assert!(cancel.sleep(Duration::from_millis(10)));
        let other = cancel.clone();
        let started = Instant::now();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            other.cancel();
        });
        assert!(!cancel.sleep(Duration::from_secs(30)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
