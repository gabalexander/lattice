//! What a command prints on standard output, for a person or a script, and
//! stopping quietly once whatever reads it has gone, as `head -1` goes in
//! `lattice status | head -1`.
//!
//! Rust ignores SIGPIPE, so a write to a pipe nobody reads any more fails
//! with `BrokenPipe` rather than killing lattice, and `println!` panics
//! over it. It stays ignored: the server writes to sockets that close under
//! it, and to the pipes of the `claude` it runs, where a signal would kill
//! it whatever it was in the middle of. Only standard output's own failure
//! stops a command: `out!` and `outln!` take the place of `print!` and
//! `println!` (clippy refuses those), and say `Closed`, which `main` exits
//! 0 for. Not 141, as a shell says of a program SIGPIPE killed: the reader
//! had what it wanted, nothing lattice was asked went wrong, and a script
//! under `set -o pipefail` shouldn't fail for it.
//!
//! What lattice says on standard error, a warning along the way or the
//! server's log, goes through `err!` and `errln!` in place of `eprint!`
//! and `eprintln!` (clippy refuses those too), which panic just the same
//! once standard error's reader has gone, as it goes in `lattice … 2>&1 |
//! head -1`. Nothing stops over it: what's said there is said beside the
//! work, not the work, and is lost.
//!
//! Adapted from crystal's `src/output.rs` (MIT).

use anyhow::Result;
use std::fmt;
use std::io::{self, Write};

/// Standard output's reader went away, which stops the command, and has
/// lattice exit 0.
#[derive(Debug)]
pub struct Closed;

impl fmt::Display for Closed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("standard output was closed")
    }
}

impl std::error::Error for Closed {}

/// `print!`, as an error rather than a panic when standard output fails:
/// `Closed` once its reader has gone.
#[macro_export]
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::output::print(format_args!($($arg)*))
    };
}

/// `println!`, as an error rather than a panic when standard output fails:
/// `Closed` once its reader has gone.
#[macro_export]
macro_rules! outln {
    () => {
        $crate::output::print(format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::output::print(format_args!("{}\n", format_args!($($arg)*)))
    };
}

/// `eprint!`, without the panic when standard error fails: what it says
/// is lost, and the command carries on.
#[macro_export]
macro_rules! err {
    ($($arg:tt)*) => {
        $crate::output::say(format_args!($($arg)*))
    };
}

/// `eprintln!`, without the panic when standard error fails: what it says
/// is lost, and the command carries on.
#[macro_export]
macro_rules! errln {
    () => {
        $crate::output::say(format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::output::say(format_args!("{}\n", format_args!($($arg)*)))
    };
}

/// Writes `args` on standard output, for `out!` and `outln!`.
pub fn print(args: fmt::Arguments<'_>) -> Result<()> {
    io::stdout().write_fmt(args).map_err(failed)
}

/// Writes `args` on standard error, for `err!` and `errln!`, and passes
/// over its failing.
pub fn say(args: fmt::Arguments<'_>) {
    let _ = io::stderr().write_fmt(args);
}

/// What a write to standard output failing comes to: `Closed` when its
/// reader has gone, or else the error, said as standard output's.
pub fn failed(err: io::Error) -> anyhow::Error {
    match err.kind() {
        io::ErrorKind::BrokenPipe => Closed.into(),
        _ => anyhow::Error::new(err).context("couldn't write to standard output"),
    }
}

/// Whether `err` is standard output closing under the command, however
/// it was carried up.
pub fn closed(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| cause.is::<Closed>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_broken_pipe_is_the_reader_gone() {
        let gone = failed(io::Error::from(io::ErrorKind::BrokenPipe));
        assert!(closed(&gone));
        assert!(closed(&gone.context("printing the repositories")));
        let full = failed(io::Error::from(io::ErrorKind::StorageFull));
        assert!(!closed(&full));
        assert!(format!("{full:#}").starts_with("couldn't write to standard output: "));
        // A page's socket closing is a failure like any other.
        let socket = anyhow::Error::new(io::Error::from(io::ErrorKind::BrokenPipe));
        assert!(!closed(&socket));
    }
}
