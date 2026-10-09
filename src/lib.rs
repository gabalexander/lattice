//! lattice: a code wiki for your own repositories. It reads a repository
//! with Claude Code, writes one page about it, an outline, sections and
//! subsections with a diagram each and every name linked to the line it's
//! defined on, keeps it up to date as the code changes, and answers
//! questions about the code in a chat; all of it on the user's machine,
//! with their own Claude subscription.
//!
//! The binary, `src/main.rs`, is its command line; everything else is
//! here, for it and for the tests. AGENTS.md has a line on each module.

pub mod cancel;
pub mod claude;
pub mod clipboard;
pub mod config;
pub mod db;
pub mod download;
pub mod links;
pub mod mermaid;
pub mod output;
pub mod paths;
pub mod printable;
pub mod secrets;
pub mod shell;
pub mod source;
pub mod time;
