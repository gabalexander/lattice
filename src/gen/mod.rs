//! The generator: what writes a version of a repository's wiki with
//! Claude Code, for a job ([`crate::generator`]). It plans the page's
//! outline, has a writer write each subsection from the code with read-only
//! tools, checks every link into the code and every diagram before it keeps
//! them, links every name the prose gives in backticks to where it's
//! defined, and writes each section's summary and the overview last; a sync
//! writes again only what the code's changes touched, and a resume carries
//! on a build that stopped. `docs/generation.md` says how it works, what it
//! costs, and what it sends where.
//!
//! - [`build`]: the phases of a build, a sync and a resume, and their caps
//! - [`plan`]: the outline, asked for, read and put in order
//! - [`write`]: what each writer is told, and what it answers
//! - [`check`]: the links checked, and the linker
//! - [`diagram`], [`fences`]: the diagrams checked, and how many a text has
//! - [`validate`]: whether an answer is a text at all, and its tidying
//! - [`files`]: the files written about; what they define, and where, is
//!   the symbol index's ([`crate::index`])
//! - [`prose`]: the links and code spans in markdown; [`repo`]: git
//! - [`book`]: `build.json`, what a build wrote and what a version was
//!   written from

pub mod book;
pub mod build;
pub mod check;
pub mod diagram;
pub mod fences;
pub mod files;
pub mod plan;
pub mod prose;
pub mod repo;
pub mod validate;
pub mod write;

use crate::cancel::Cancel;
use crate::generator::{Built, Generator, JobSpec, Report};
use anyhow::Result;

/// The generator that writes with Claude Code.
pub struct Claude;

impl Generator for Claude {
    fn run(&self, job: &JobSpec, report: &mut dyn FnMut(Report), cancel: &Cancel) -> Result<Built> {
        build::run(job, report, cancel)
    }
}
