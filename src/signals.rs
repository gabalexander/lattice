//! Stopping on ctrl+c or SIGTERM. The `claude` a job runs is in a process
//! group of its own, which a terminal's ctrl+c doesn't reach and which
//! would carry on spending without anyone to read it; so lattice catches
//! the signal and stops its work itself, through a [`Cancel`], before it
//! exits.

use crate::cancel::Cancel;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Whether a signal asked lattice to stop.
static ASKED: AtomicBool = AtomicBool::new(false);

static INSTALLED: Once = Once::new();

/// What runs when SIGINT or SIGTERM comes: it only notes it, as a signal
/// handler may do nothing else safely.
extern "C" fn asked(_: libc::c_int) {
    ASKED.store(true, Ordering::SeqCst);
}

/// Catches SIGINT and SIGTERM from now on, for [`asked_to_stop`] to say.
pub fn catch() {
    INSTALLED.call_once(|| {
        let handler = asked as extern "C" fn(libc::c_int) as libc::sighandler_t;
        // SAFETY: the handler only stores to an atomic, which is
        // async-signal-safe.
        unsafe {
            libc::signal(libc::SIGINT, handler);
            libc::signal(libc::SIGTERM, handler);
        }
    });
}

/// Whether SIGINT or SIGTERM has come since [`catch`].
pub fn asked_to_stop() -> bool {
    ASKED.load(Ordering::SeqCst)
}

/// Cancels `cancel` once SIGINT or SIGTERM comes.
pub fn cancel_on_stop(cancel: Cancel) {
    catch();
    thread::spawn(move || {
        while !asked_to_stop() {
            thread::sleep(Duration::from_millis(100));
        }
        cancel.cancel();
    });
}
