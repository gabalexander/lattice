//! Writing a version of a wiki: what a job's build, regeneration, sync and
//! resume do.
//!
//! A build reads a tree of the commit, lists its files and indexes what
//! they define, checks Claude Code can read them, then goes through four
//! phases, each told to the job as its progress:
//!
//! 1. **Plan.** One Claude reads the repository and plans the outline
//!    ([`super::plan`]).
//! 2. **Write.** One writer a subsection, `concurrency` at once, each
//!    starting from its seed files and exploring with read-only tools
//!    ([`super::write`]); what it writes is checked, its links and its
//!    diagrams ([`super::check`], [`super::diagram`]), sent back once with
//!    what's wrong, what's still wrong dropped, then linked.
//! 3. **Link.** Every subsection checked once more against the page as it
//!    turned out, its anchors and the commit, and linked.
//! 4. **Overview.** Each section's summary from its subsections, then the
//!    overview from the sections.
//!
//! Everything written is kept in `build.json` as it's written, so a build
//! stopped halfway, cancelled, out of budget, out of time, or failed,
//! carries on where it was when it's resumed. Each run of Claude has its
//! own caps on what it spends, how many turns it takes and how long; the
//! build has the settings' budget and [`BUILD_TIME`]: past either it starts
//! no more runs, and stops.
//!
//! A sync writes again only the subsections whose files changed since the
//! version it starts from, giving each writer its text before and the diff
//! (it may say nothing needs to change), and the summaries and the overview
//! only where a writer says the meaning moved. It plans again when much
//! changed, or when many source files came that no subsection covers,
//! keeping every subsection whose files are as they were. The links of
//! what isn't written again are moved with the lines they point at.
//!
//! Adapted from crystal's wiki generator (`src/wiki/build.rs`, MIT), with
//! the sync, the resume and the caps of deepwiki-by-cc's
//! `src/lib/server/queue/handlers.ts` (MIT; see THIRD_PARTY_NOTICES.md).

use super::book::{Book, Built as Kept, Page, Run, Summary};
use super::check::{self, Tally};
use super::diagram::{self, Room};
use super::files::Files;
use super::plan::{self, Plan, PlannedSection, PlannedSubsection};
use super::repo::{self, Remap};
use super::validate;
use super::write::{self, Written};
use crate::cancel::Cancel;
use crate::claude::{self, Ask, Tools};
use crate::config::Config;
use crate::db::{JobKind, Phase, Progress};
use crate::generator::{Built, JobSpec, Report};
use crate::index::Index;
use crate::paths;
use crate::source::Source;
use crate::time::Timestamp;
use crate::wiki::{self, CodeLink, Generated, Overview, Section, Subsection, Wiki};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// What one run of Claude may spend, how many turns it may take, and how
/// long it may last.
#[derive(Debug, Clone, Copy)]
struct Caps {
    budget_usd: f64,
    turns: u32,
    time: Duration,
}

/// The planner's caps.
const PLAN: Caps = Caps {
    budget_usd: 3.0,
    turns: 60,
    time: Duration::from_secs(20 * 60),
};

/// A subsection writer's caps.
const WRITE: Caps = Caps {
    budget_usd: 1.2,
    turns: 40,
    time: Duration::from_secs(15 * 60),
};

/// The caps of a writer asked to put its text right, and of the planner
/// asked to put its outline right.
const FIX: Caps = Caps {
    budget_usd: 0.6,
    turns: 16,
    time: Duration::from_secs(8 * 60),
};

/// A section summary writer's caps.
const SECTION: Caps = Caps {
    budget_usd: 0.8,
    turns: 16,
    time: Duration::from_secs(10 * 60),
};

/// The overview writer's caps.
const OVERVIEW: Caps = Caps {
    budget_usd: 1.2,
    turns: 20,
    time: Duration::from_secs(15 * 60),
};

/// How many times a run that failed for a passing reason, Claude
/// overloaded or rate limited, is tried again.
const RETRIES: u32 = 2;

/// The longest one run of a build may take, every phase together: past
/// it, the build starts no more runs of Claude, and stops for a resume.
pub const BUILD_TIME: Duration = Duration::from_secs(4 * 60 * 60);

/// The least a run of Claude is started with: below it, the build stops
/// for its budget.
const LEAST_CALL: f64 = 0.15;

/// How much of the diff of its files a sync's writer is given.
const DIFF_SHOWN: usize = 40 * 1024;

/// A sync plans again when more than this share of the subsections
/// changed.
const REPLAN_SHARE: f64 = 0.5;

/// Or when more source files than this came that no subsection covers.
const REPLAN_UNCOVERED: usize = 12;

/// A build with more than this share of its subsections unwritten fails,
/// for a resume to write them, rather than make a version without them.
const MOST_MISSING: f64 = 0.2;

/// How often the threads' reports are passed on while they run.
const PUMP: Duration = Duration::from_millis(100);

/// How much of a README the planner is given is the planner's business;
/// this is how much is read.
const README_READ: u64 = 64 * 1024;

/// What the threads of a build share: the book, written down as it
/// changes, the money, and where their reports go.
struct Shared<'a> {
    state: Mutex<State>,
    book_path: PathBuf,
    tx: Sender<Report>,
    config: &'a Config,
    tree: &'a Path,
    cancel: &'a Cancel,
    started: Instant,
    /// Whether Claude Code can read the tree with the flags it's given,
    /// checked once, before the first run of the build spends anything.
    preflight: OnceLock<Result<(), String>>,
}

struct State {
    book: Book,
    /// What this job has spent.
    spent: f64,
    /// What the runs of Claude under way may still spend.
    reserved: f64,
    /// Why it stopped starting runs, once it has.
    stopped: Option<String>,
    /// The run's seconds before this job.
    seconds_before: u64,
    progress: Progress,
}

/// What a text is checked and written with: the files, what they define,
/// and where else the code is on this machine.
struct Code<'a> {
    files: &'a Files,
    index: &'a Index,
    elsewhere: &'a [PathBuf],
}

/// Runs `job`: writes the version's `wiki.json` and `build.json` in
/// `job.work`, telling `report` how it goes.
pub fn run(job: &JobSpec, report: &mut dyn FnMut(Report), cancel: &Cancel) -> Result<Built> {
    let book_path = job.work.join(paths::BUILD_FILE);
    let wiki_path = job.work.join(paths::WIKI_FILE);
    std::fs::create_dir_all(&job.work)
        .with_context(|| format!("couldn't make {}", job.work.display()))?;
    let mut book = match job.kind {
        JobKind::Resume => Book::read(&book_path)?,
        _ => Book::default(),
    };
    // A build that was done before its job could keep it is done.
    if let Some(kept) = &book.built
        && wiki_path.exists()
    {
        report(Report::Log("the build was done already: keeping it".into()));
        return Ok(Built {
            model: kept.model.clone(),
            cost_usd: kept.cost_usd,
        });
    }
    let base = match (job.kind, &job.latest) {
        (JobKind::Sync, Some(latest)) => {
            let dir = paths::version_dir(&job.repo.key, latest.n);
            let kept = Book::read(&dir.join(paths::BUILD_FILE))?.built;
            let wiki = Wiki::read(&dir.join(paths::WIKI_FILE)).ok();
            match (kept, wiki) {
                (Some(kept), Some(wiki)) => Some((latest.n, kept, wiki)),
                _ => {
                    report(Report::Log(format!(
                        "version {} has no outline to sync from: building it again",
                        latest.n
                    )));
                    None
                }
            }
        }
        _ => None,
    };
    let run = match (job.kind, book.run.take()) {
        (JobKind::Resume, Some(run)) => {
            report(Report::Log(format!(
                "carrying on with the {} of {} stopped at {}",
                kind_word(run.kind),
                short(&run.commit),
                phase_word(run.phase)
            )));
            run
        }
        (JobKind::Resume, None) => {
            bail!("there's no stopped build to resume: build or sync instead")
        }
        (JobKind::Sync, _) if base.is_some() => Run::new(
            JobKind::Sync,
            &job.commit,
            job.branch.clone(),
            base.as_ref().map(|(n, _, _)| *n),
        ),
        _ => Run::new(JobKind::Build, &job.commit, job.branch.clone(), None),
    };
    // A sync resumed starts from the version it started from.
    let base = match (&base, run.base) {
        (None, Some(n)) => {
            let dir = paths::version_dir(&job.repo.key, n);
            let kept = Book::read(&dir.join(paths::BUILD_FILE))?.built;
            let wiki = Wiki::read(&dir.join(paths::WIKI_FILE)).ok();
            kept.zip(wiki).map(|(kept, wiki)| (n, kept, wiki))
        }
        _ => base,
    };
    let tree = repo::tree(
        &job.root,
        &run.commit,
        &paths::repo_dir(&job.repo.key).join("tree"),
    )?;
    let files = Files::list(&tree, &job.config.exclude)?;
    let index = Index::build_cancellable(
        &tree,
        &run.commit,
        &paths::index_dir(&tree),
        &job.config.index_settings(),
        cancel,
    )?;
    let (tx, rx) = mpsc::channel();
    let seconds_before = run.seconds;
    let progress = Progress {
        phase: run.phase,
        done: 0,
        total: 1,
        current: None,
        cost_usd: round_cents(run.cost_usd),
    };
    book.run = Some(run);
    let shared = Shared {
        state: Mutex::new(State {
            book,
            spent: 0.0,
            reserved: 0.0,
            stopped: None,
            seconds_before,
            progress,
        }),
        book_path,
        tx,
        config: &job.config,
        tree: &tree,
        cancel,
        started: Instant::now(),
        preflight: OnceLock::new(),
    };
    shared.log(&format!(
        "{} of {} at {} with {}: {} files, {} of them source, {} lines of source; {}",
        kind_word(shared.run(|run| run.kind)),
        job.repo.name,
        short(&shared.run(|run| run.commit.clone())),
        job.config.model,
        files.files.len(),
        files.sources().count(),
        files.source_lines(),
        if job.config.budget_usd > 0.0 {
            format!("budget ${:.2}", job.config.budget_usd)
        } else {
            "no budget".to_string()
        },
    ));
    // How each language was indexed, which says how far its links reach.
    for line in index.summary() {
        shared.log(&line);
    }
    shared.save();
    let mut elsewhere = vec![job.root.clone()];
    if let Source::Local { path } = &job.repo.source {
        elsewhere.push(path.clone());
    }
    let page = Code {
        files: &files,
        index: &index,
        elsewhere: &elsewhere,
    };
    let result = build(&shared, &rx, report, job, &page, base.as_ref());
    shared.flush(&rx, report);
    let mut state = shared.state.into_inner().expect("no thread panicked");
    match result {
        Ok(mut wiki) => {
            let run = state.book.run.take().expect("the run is there");
            let model = run.top_model().unwrap_or_else(|| job.config.model.clone());
            wiki.generated.model = model.clone();
            wiki.generated.by = display_name(&model);
            wiki.generated.cost_usd = round_cents(run.cost_usd);
            wiki.write(&wiki_path)?;
            let missing: BTreeSet<String> = run
                .plan
                .as_ref()
                .map(|plan| {
                    plan.subsections()
                        .filter(|(_, sub)| wiki.subsection(&sub.id).is_none())
                        .map(|(_, sub)| sub.id.clone())
                        .collect()
                })
                .unwrap_or_default();
            let plan = run.plan.clone().expect("planned");
            let blobs = plan
                .subsections()
                .filter(|(_, sub)| !missing.contains(&sub.id))
                .map(|(_, sub)| (sub.id.clone(), blobs_of(&files, &sub.files)))
                .collect();
            let seconds = state.seconds_before + shared.started.elapsed().as_secs();
            state.book.built = Some(Kept {
                commit: run.commit.clone(),
                branch: run.branch.clone(),
                at: Timestamp::now(),
                plan,
                blobs,
                missing: missing.clone(),
                cost_usd: run.cost_usd,
                model: model.clone(),
                tally: run.tally,
                seconds,
            });
            state.book.write(&job.work.join(paths::BUILD_FILE))?;
            report(Report::Log(format!(
                "wrote {} sections, {} subsections ({} written now{}), {} diagrams, {} links into \
                 the code; ${:.2} in {}",
                wiki.sections.len(),
                wiki.subsections(),
                written(&run),
                if missing.is_empty() {
                    String::new()
                } else {
                    format!(", {} couldn't be: a sync tries them again", missing.len())
                },
                wiki.diagrams(),
                wiki.code_links(),
                run.cost_usd,
                duration(seconds),
            )));
            let tally = run.tally;
            report(Report::Log(format!(
                "the checks moved {} links and dropped {}, rewrote {} diagrams and left out {}, \
                 sent {} texts back and wrote {} again; the linker linked {} code spans",
                tally.moved,
                tally.dropped,
                tally.diagrams_fixed,
                tally.diagrams_dropped,
                tally.fixes,
                tally.rewritten,
                tally.linked
            )));
            Ok(Built {
                model,
                cost_usd: round_cents(run.cost_usd),
            })
        }
        Err(err) => {
            if let Some(run) = state.book.run.as_mut() {
                run.seconds = state.seconds_before + shared.started.elapsed().as_secs();
            }
            let _ = state.book.write(&job.work.join(paths::BUILD_FILE));
            Err(err)
        }
    }
}

/// The build itself, from the plan to the wiki, which its caller writes.
fn build(
    shared: &Shared,
    rx: &Receiver<Report>,
    report: &mut dyn FnMut(Report),
    job: &JobSpec,
    page: &Code,
    base: Option<&(u32, Kept, Wiki)>,
) -> Result<Wiki> {
    let name = job.repo.name.as_str();
    // The outline, and what to write.
    if shared.run(|run| run.plan.is_none()) {
        shared.progress(Phase::Plan, 0, 1, Some("planning the outline"));
        let kind = shared.run(|run| run.kind);
        let result = Mutex::new(None);
        pool(shared, rx, report, vec![()], &|()| {
            *result.lock().unwrap() = Some(match (kind, base) {
                (JobKind::Sync, Some((_, kept, wiki))) => {
                    decide_sync(shared, name, page, kept, wiki)
                }
                _ => make_plan(shared, name, page.files).map(|plan| {
                    shared.change(|run| {
                        run.todo = plan.subsections().map(|(_, sub)| sub.id.clone()).collect();
                        run.plan = Some(plan);
                        run.replanned = true;
                    });
                }),
            });
        });
        let planned = result.into_inner().expect("no thread panicked");
        planned.unwrap_or_else(|| Err(stopped(shared)))?;
        shared.progress(Phase::Plan, 1, 1, None);
    }
    let plan = shared.run(|run| run.plan.clone()).expect("planned");
    let anchors = plan.anchors();
    let earlier_wiki = base.map(|(_, _, wiki)| wiki);
    let earlier_commit = base.map(|(_, kept, _)| kept.commit.clone());
    // Where the lines the version's links point at are now.
    let remap = match &earlier_commit {
        Some(before) if *before != shared.run(|run| run.commit.clone()) => {
            Remap::between(shared.tree, before, &shared.run(|run| run.commit.clone()))
                .unwrap_or_default()
        }
        _ => Remap::default(),
    };
    // The subsections.
    let todo: Vec<(PlannedSection, PlannedSubsection)> = plan
        .subsections()
        .filter(|(_, sub)| {
            shared.run(|run| run.todo.contains(&sub.id) && !run.pages.contains_key(&sub.id))
        })
        .map(|(section, sub)| (section.clone(), sub.clone()))
        .collect();
    let total = shared.run(|run| run.todo.len()) as u32;
    let done = shared.run(written) as u32;
    shared.progress(Phase::Write, done, total, None);
    if !todo.is_empty() {
        shared.log(&format!(
            "writing {} subsections, {} at once",
            todo.len(),
            shared.config.concurrency
        ));
    }
    pool(shared, rx, report, todo, &|(section, sub)| {
        shared.progress_current(&sub.title);
        let started = Instant::now();
        let earlier = earlier_wiki.and_then(|wiki| wiki.subsection(&sub.id));
        let update = earlier.zip(earlier_commit.as_deref());
        let update = update.map(|(before, commit)| (before, commit, &remap));
        match write_subsection(shared, name, &plan, &section, &sub, page, &anchors, update) {
            Ok((written, tally)) => {
                let cost = written.cost_usd;
                let done = shared.change(|run| {
                    run.tally += tally;
                    run.failed.remove(&sub.id);
                    run.pages.insert(sub.id.clone(), written);
                    self::written(run)
                });
                shared.progress(Phase::Write, done as u32, total, Some(&sub.title));
                shared.log(&format!(
                    "[{done}/{total}] {} (${cost:.2}, {}s)",
                    sub.title,
                    started.elapsed().as_secs()
                ));
            }
            Err(err) => {
                let why = format!("{err:#}");
                shared.log(&format!("couldn't write {}: {why}", sub.title));
                shared.change(|run| {
                    run.failed.insert(sub.id.clone(), why);
                });
            }
        }
    });
    shared.halt(&format!(
        "{} of {total} subsections are written, and kept",
        shared.run(written)
    ))?;
    let missing = shared.run(|run| {
        run.todo
            .iter()
            .filter(|id| !run.pages.contains_key(*id))
            .count()
    });
    let all = plan.subsections().count();
    if missing > 0 && missing as f64 > all as f64 * MOST_MISSING {
        bail!(
            "{missing} of {all} subsections couldn't be written (the log says why): a resume \
             tries them again"
        );
    }
    // Every subsection checked against the page as it turned out, and
    // linked.
    let mut sections = assemble(shared, &plan, earlier_wiki, &remap)?;
    let anchors = page_anchors(&sections);
    let all = sections.iter().map(|s| s.subsections.len()).sum::<usize>() as u32;
    shared.progress(Phase::Link, 0, all, None);
    let mut linked = 0;
    let mut tally = Tally::default();
    for section in &mut sections {
        for sub in &mut section.subsections {
            let (text, more) = final_check(&sub.body_md, page, shared.tree, &anchors, &sub.files);
            sub.body_md = text;
            tally += more;
            linked += 1;
            shared.progress(Phase::Link, linked, all, Some(&sub.title));
        }
    }
    shared.change(|run| run.tally += tally);
    shared.flush(rx, report);
    // The sections' summaries, then the overview.
    let to_write = sections_to_write(shared, &plan, &sections, earlier_wiki);
    // The overview last, counted though a sync may find it needn't be
    // written once the summaries are.
    let total = to_write.len() as u32 + 1;
    let mut done = 0;
    shared.progress(Phase::Overview, done, total, None);
    let finished = Mutex::new(done);
    let built_sections = &sections;
    pool(shared, rx, report, to_write, &|section_id: String| {
        let section = built_sections
            .iter()
            .find(|s| s.id == section_id)
            .expect("a section of the page");
        let planned = plan
            .sections
            .iter()
            .find(|s| s.id == section_id)
            .expect("a planned section");
        shared.progress_current(&section.title);
        let earlier = earlier_wiki.and_then(|wiki| wiki.section(&section_id));
        match write_section(
            shared, name, &plan, planned, section, page, &anchors, earlier,
        ) {
            Ok((summary, tally)) => {
                let cost = summary.cost_usd;
                shared.change(|run| {
                    run.tally += tally;
                    run.sections.insert(section_id.clone(), summary);
                });
                let mut finished = finished.lock().unwrap();
                *finished += 1;
                shared.progress(Phase::Overview, *finished, total, Some(&section.title));
                shared.log(&format!("the section {} (${cost:.2})", section.title));
            }
            Err(err) => shared.log(&format!("couldn't summarize {}: {err:#}", section.title)),
        }
    });
    done = *finished.lock().unwrap();
    shared.halt("the subsections are written, and kept")?;
    // Each summary, written now or kept from the version, checked against
    // the page as it is now.
    for section in &mut sections {
        if let Some(summary) = shared.run(|run| run.sections.get(&section.id).cloned()) {
            section.summary_md = summary.summary_md;
            section.diagram = summary.diagram;
        }
        let near: Vec<String> = section
            .subsections
            .iter()
            .flat_map(|sub| sub.files.iter().cloned())
            .collect();
        let (text, more) = final_check(&section.summary_md, page, shared.tree, &anchors, &near);
        section.summary_md = text;
        shared.change(|run| run.tally += more);
    }
    if overview_to_write(shared, &plan, earlier_wiki) {
        shared.progress(Phase::Overview, done, total, Some("the overview"));
        let result = Mutex::new(None);
        let built_sections = &sections;
        pool(shared, rx, report, vec![()], &|()| {
            *result.lock().unwrap() = Some(write_overview(
                shared,
                name,
                &plan,
                built_sections,
                page,
                &anchors,
                earlier_wiki,
            ));
        });
        let written = result.into_inner().expect("no thread panicked");
        match written.unwrap_or_else(|| Err(stopped(shared))) {
            Ok((summary, tally)) => {
                let cost = summary.cost_usd;
                shared.change(|run| {
                    run.tally += tally;
                    run.overview = Some(summary);
                });
                shared.log(&format!("the overview (${cost:.2})"));
            }
            Err(err) if shared.stopped() || shared.cancel.is_cancelled() => return Err(err),
            Err(err) => shared.log(&format!("couldn't write the overview: {err:#}")),
        }
    }
    shared.progress(Phase::Overview, total, total, None);
    shared.halt("everything but the overview is written, and kept")?;
    let run = shared.run(Clone::clone);
    let overview = match &run.overview {
        Some(summary) => Overview {
            summary_md: summary.summary_md.clone(),
            diagram: summary.diagram.clone(),
        },
        None => earlier_wiki.map_or_else(Overview::default, |wiki| Overview {
            summary_md: moved(&wiki.overview.summary_md, &remap),
            diagram: wiki.overview.diagram.clone(),
        }),
    };
    let (summary_md, more) = final_check(&overview.summary_md, page, shared.tree, &anchors, &[]);
    shared.change(|run| run.tally += more);
    let overview = Overview {
        summary_md,
        ..overview
    };
    let (web_url, code_url) = match web_of(job) {
        Some(web) => (Some(web.url), Some(web.code_url)),
        None => (None, None),
    };
    Ok(Wiki {
        version: wiki::VERSION,
        repo: wiki::Repo {
            name: name.to_string(),
            root: match &job.repo.source {
                Source::Local { path } => Some(path.clone()),
                Source::Git { .. } => None,
            },
            commit: run.commit.clone(),
            branch: run.branch.clone(),
            web_url,
            code_url,
        },
        generated: Generated {
            at: Timestamp::now().to_string(),
            by: String::new(),
            model: String::new(),
            cost_usd: 0.0,
            lattice: env!("CARGO_PKG_VERSION").to_string(),
        },
        overview,
        sections,
    })
}

/// How many of the subsections `run` writes are written.
fn written(run: &Run) -> usize {
    run.todo
        .iter()
        .filter(|id| run.pages.contains_key(*id))
        .count()
}

/// The error a step that never ran gives: the build stopped, or was
/// cancelled, before it.
fn stopped(shared: &Shared) -> anyhow::Error {
    match shared.stopped_why() {
        Some(why) => anyhow!("{why}"),
        None => anyhow!("the build was cancelled; what it wrote is kept for a resume"),
    }
}

/// For a sync, what to write again, from what changed since the version
/// `kept` and `wiki`: the outline kept with the source files nobody covered
/// given to the subsections nearest them, or, when much changed, a new one.
fn decide_sync(shared: &Shared, name: &str, page: &Code, kept: &Kept, wiki: &Wiki) -> Result<()> {
    let files = page.files;
    let mut plan = kept.plan.clone();
    let changed = changed(kept, wiki, files);
    let all = plan.subsections().count();
    let uncovered = plan.uncovered(files).len();
    if replans(changed.len(), all, uncovered) {
        shared.log(&format!(
            "{} of {all} subsections changed and {uncovered} new source files have no subsection: \
             planning again",
            changed.len()
        ));
        let fresh = make_plan(shared, name, files)?;
        // A subsection whose files are as they were is kept as it was.
        let same: BTreeMap<String, String> = fresh
            .subsections()
            .filter_map(|(_, sub)| {
                let blobs = blobs_of(files, &sub.files);
                let was = kept.plan.subsections().find(|(_, before)| {
                    before.files == sub.files && kept.blobs.get(&before.id) == Some(&blobs)
                })?;
                Some((sub.id.clone(), was.1.id.clone()))
            })
            .collect();
        let todo = fresh
            .subsections()
            .filter(|(_, sub)| !same.contains_key(&sub.id))
            .map(|(_, sub)| sub.id.clone())
            .collect();
        shared.change(|run| {
            for (id, was) in &same {
                if let Some(before) = wiki.subsection(was) {
                    run.pages.insert(
                        id.clone(),
                        Page {
                            body_md: before.body_md.clone(),
                            diagram: before.diagram.clone(),
                            blobs: kept.blobs.get(was).cloned().unwrap_or_default(),
                            meaning_changed: false,
                            cost_usd: 0.0,
                        },
                    );
                }
            }
            run.todo = todo;
            run.plan = Some(fresh);
            run.replanned = true;
        });
        return Ok(());
    }
    let mut todo = changed;
    if uncovered > 0 {
        let before: BTreeMap<String, Vec<String>> = plan
            .subsections()
            .map(|(_, sub)| (sub.id.clone(), sub.files.clone()))
            .collect();
        plan.cover(files);
        for (_, sub) in plan.subsections() {
            if before.get(&sub.id) != Some(&sub.files) {
                todo.insert(sub.id.clone());
            }
        }
    }
    // A subsection whose files are all gone goes, and a section left with
    // none.
    let mut gone = false;
    for section in &mut plan.sections {
        section.subsections.retain(|sub| {
            let keep = sub
                .files
                .iter()
                .any(|entry| entry.is_empty() || files.entry(entry).is_some());
            gone |= !keep;
            keep
        });
        for sub in &mut section.subsections {
            sub.files
                .retain(|entry| entry.is_empty() || files.entry(entry).is_some());
        }
    }
    plan.sections
        .retain(|section| !section.subsections.is_empty());
    todo.retain(|id| plan.subsection(id).is_some());
    shared.log(&format!(
        "{} of {} subsections changed since {}",
        todo.len(),
        plan.subsections().count(),
        short(&kept.commit)
    ));
    shared.change(|run| {
        run.todo = todo;
        run.plan = Some(plan);
        run.replanned = gone;
    });
    Ok(())
}

/// The subsections of the version `kept` and `wiki` that a sync writes
/// again: those whose files changed, by their hashes, and those it couldn't
/// write.
fn changed(kept: &Kept, wiki: &Wiki, files: &Files) -> BTreeSet<String> {
    kept.plan
        .subsections()
        .filter(|(_, sub)| {
            kept.missing.contains(&sub.id)
                || wiki.subsection(&sub.id).is_none()
                || kept.blobs.get(&sub.id) != Some(&blobs_of(files, &sub.files))
        })
        .map(|(_, sub)| sub.id.clone())
        .collect()
}

/// Whether a sync plans the outline again, `changed` of its `all`
/// subsections changed and `uncovered` source files covered by none.
fn replans(changed: usize, all: usize, uncovered: usize) -> bool {
    let much = all > 0 && changed as f64 / all as f64 > REPLAN_SHARE;
    much || uncovered > REPLAN_UNCOVERED
}

/// Has the planner plan the wiki, sends its outline back once when it's
/// badly off, and gives the source files still left out to the subsections
/// nearest them.
fn make_plan(shared: &Shared, name: &str, files: &Files) -> Result<Plan> {
    shared.log("planning the outline");
    let started = Instant::now();
    let commit = shared.run(|run| run.commit.clone());
    let ask = shared.ask_of(
        plan::SYSTEM.to_string(),
        plan::message(name, &commit, files, readme(shared.tree, files).as_deref()),
        plan::schema(),
        PLAN,
    );
    let answer = shared.ask(ask.clone(), "the planner")?;
    let value = answer.value.unwrap_or_default();
    let mut read = plan::read(&value, files)?;
    if !read.problems.is_empty() {
        shared.log(&format!(
            "the outline's problems: {}",
            read.problems.join("; ")
        ));
        let fix = Ask {
            message: plan::fix_message(&value, &read.problems, files),
            ..shared.with_caps(ask, FIX)
        };
        match shared.ask(fix, "the planner, putting its outline right") {
            Ok(fixed) => match plan::read(&fixed.value.unwrap_or_default(), files) {
                Ok(fixed) => read = fixed,
                Err(err) => shared.log(&format!("its outline put right didn't read: {err:#}")),
            },
            Err(err) => shared.log(&format!("couldn't have the outline put right: {err:#}")),
        }
    }
    for note in &read.notes {
        shared.log(&format!("the outline: {note}"));
    }
    let mut plan = read.plan;
    let given = plan.cover(files);
    if given > 0 {
        shared.log(&format!(
            "{given} source files left out went to the subsections nearest them"
        ));
    }
    shared.log(&format!(
        "planned {} sections, {} subsections ({}s)",
        plan.sections.len(),
        plan.subsections().count(),
        started.elapsed().as_secs()
    ));
    Ok(plan)
}

/// The repository's README, when it has one, for the planner.
fn readme(tree: &Path, files: &Files) -> Option<String> {
    let path = files
        .files
        .keys()
        .find(|path| !path.contains('/') && path.to_ascii_lowercase().starts_with("readme"))?;
    let file = std::fs::File::open(tree.join(path)).ok()?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(file, README_READ), &mut text).ok()?;
    Some(crate::secrets::redact(&text))
}

/// Writes one subsection, or for a sync, writes it again from `update`:
/// its text before, the commit it was written at, and where the lines its
/// links point at are now. The writer, the checks, the writer again with
/// what's wrong, the checks once more, then the linker.
#[allow(clippy::too_many_arguments)]
fn write_subsection(
    shared: &Shared,
    name: &str,
    plan: &Plan,
    section: &PlannedSection,
    sub: &PlannedSubsection,
    page: &Code,
    anchors: &BTreeSet<String>,
    update: Option<(&Subsection, &str, &Remap)>,
) -> Result<(Page, Tally)> {
    let task = write::subsection_task(name, section, sub);
    let mut message = write::subsection_message(&task, plan, sub);
    let mut system = write::subsection_system();
    if let Some((before, commit, _)) = update {
        let now = shared.run(|run| run.commit.clone());
        message.push_str(&write::update_part(
            &before.body_md,
            before.diagram.as_ref(),
            &repo::diff(shared.tree, commit, &now, &sub.files, DIFF_SHOWN),
            &repo::commits_between(shared.tree, commit, &now, &sub.files, 30),
        ));
        system.push_str("\n\n");
        system.push_str(write::UPDATE);
    }
    message.push_str(&write::seeds(
        shared.tree,
        page.files,
        page.index,
        &sub.files,
    ));
    let ask = shared.ask_of(
        system,
        message,
        write::schema(false, update.is_some()),
        WRITE,
    );
    let ctx = check::Context {
        root: shared.tree,
        elsewhere: page.elsewhere,
        files: page.files,
        index: page.index,
        anchors,
        near: &sub.files,
    };
    let what = format!("the writer of {}", sub.id);
    let (written, cost, mut tally) = write_checked(
        shared,
        ask,
        &task,
        &sub.title,
        &ctx,
        Room::Part,
        false,
        &what,
    )?;
    let blobs = blobs_of(page.files, &sub.files);
    if written.unchanged
        && let Some((before, _, remap)) = update
    {
        return Ok((
            Page {
                body_md: moved(&before.body_md, remap),
                diagram: before.diagram.clone(),
                blobs,
                meaning_changed: false,
                cost_usd: cost,
            },
            tally,
        ));
    }
    let (body, more) = check::link(
        &super::prose::demote_headings(&written.text),
        page.index,
        &sub.files,
    );
    tally += more;
    Ok((
        Page {
            body_md: body,
            diagram: written.diagram,
            blobs,
            meaning_changed: written.meaning_changed,
            cost_usd: cost,
        },
        tally,
    ))
}

/// Runs a writer, checks what it wrote, and sends it back once with
/// what's wrong; what's still wrong then is dropped. A writer that fails,
/// or answers with no text, is run once more with more room first. Gives
/// back the text and diagram checked, what it all cost, and what the
/// checks did.
#[allow(clippy::too_many_arguments)]
fn write_checked(
    shared: &Shared,
    ask: Ask,
    task: &str,
    title: &str,
    ctx: &check::Context,
    room: Room,
    summary: bool,
    what: &str,
) -> Result<(Written, f64, Tally)> {
    let mut tally = Tally::default();
    let mut cost = 0.0;
    let mut written = None;
    for attempt in 0..2 {
        let this = if attempt == 0 {
            ask.clone()
        } else {
            Ask {
                max_turns: ask.max_turns.map(|turns| turns + turns / 2),
                budget_usd: ask.budget_usd * 1.5,
                ..ask.clone()
            }
        };
        let answered = shared.ask(this, what);
        let why = match answered {
            Ok(answer) => {
                cost += answer.cost_usd;
                let mut got = write::read(&answer.value.unwrap_or_default());
                got.text = validate::tidy(&got.text, title);
                if got.unchanged {
                    return Ok((got, cost, tally));
                }
                match validate::text(&got.text, got.failure.as_deref()) {
                    Ok(()) => {
                        written = Some(got);
                        break;
                    }
                    Err(no_text) => {
                        tally.rewritten += 1;
                        anyhow!("{no_text}")
                    }
                }
            }
            Err(err) => err,
        };
        if shared.stopped() || shared.cancel.is_cancelled() || attempt == 1 {
            return Err(why.context(format!("{what} failed")));
        }
        shared.log(&format!("{what} is run once more: {why:#}"));
    }
    let written = written.expect("a text, or an error before");
    let (first, problems, more) = checked(&written, ctx, room, false);
    tally += more;
    if problems.is_empty() {
        let (last, _, more) = checked(&first, ctx, room, true);
        tally += more;
        return Ok((last, cost, tally));
    }
    shared.log(&format!(
        "{what}: {} problems: {}",
        problems.len(),
        problems.join(" | ")
    ));
    tally.fixes += 1;
    let diagram_wrong = problems.iter().any(|p| p.starts_with("the diagram"));
    let fix = Ask {
        system: write::fix_system(&ask.system),
        message: write::fix_message(task, &written, &problems, summary),
        ..shared.with_caps(ask, FIX)
    };
    let fixed = match shared.ask(fix, &format!("{what}, putting it right")) {
        Ok(answer) => {
            cost += answer.cost_usd;
            let mut fixed = write::read(&answer.value.unwrap_or_default());
            fixed.text = validate::tidy(&fixed.text, title);
            validate::text(&fixed.text, fixed.failure.as_deref())
                .is_ok()
                .then_some(Written {
                    // A sync's writer said whether its meaning changed the
                    // first time.
                    meaning_changed: written.meaning_changed,
                    ..fixed
                })
        }
        Err(err) => {
            shared.log(&format!("{what} couldn't put it right: {err:#}"));
            None
        }
    };
    let (last, _, more) = checked(fixed.as_ref().unwrap_or(&first), ctx, room, true);
    tally += more;
    if diagram_wrong && last.diagram.is_some() && fixed.is_some() {
        tally.diagrams_fixed += 1;
    }
    Ok((last, cost, tally))
}

/// `written` checked: its text, its card and the diagrams in its text;
/// with `last`, what's still wrong dropped; without, listed.
fn checked(
    written: &Written,
    ctx: &check::Context,
    room: Room,
    last: bool,
) -> (Written, Vec<String>, Tally) {
    let text = check::text(&written.text, ctx, last);
    let mut problems = text.problems;
    let mut tally = text.tally;
    let card = match &written.diagram {
        None if !last => {
            problems.push("the diagram card is missing: draw one".to_string());
            None
        }
        None => None,
        Some(card) => match diagram::check(card) {
            Ok(card) => Some(card),
            Err(_) if last => {
                tally.diagrams_dropped += 1;
                None
            }
            Err(why) => {
                problems.push(format!("the diagram card doesn't read: {why}"));
                Some(card.clone())
            }
        },
    };
    let inline = diagram::inline(&text.text, card.as_ref(), room, last);
    problems.extend(inline.problems);
    tally.diagrams_dropped += inline.dropped;
    (
        Written {
            text: inline.text,
            diagram: card,
            ..written.clone()
        },
        problems,
        tally,
    )
}

/// Writes a section's summary from its subsections as they turned out.
#[allow(clippy::too_many_arguments)]
fn write_section(
    shared: &Shared,
    name: &str,
    plan: &Plan,
    planned: &PlannedSection,
    section: &Section,
    page: &Code,
    anchors: &BTreeSet<String>,
    earlier: Option<&Section>,
) -> Result<(Summary, Tally)> {
    let task = format!(
        "Write the summary of the section \"{}\" (#{}) in the wiki of {name}.",
        section.title, section.id
    );
    let mut message = format!(
        "{task}\n\nWhat it covers: {}\n\nThe outline of the whole page:\n\n{}\n\nIts \
         subsections, as their writers wrote them:\n",
        planned.about,
        plan.outline()
    );
    for sub in &section.subsections {
        message.push_str(&format!(
            "\n## {} (#{})\n\n{}\n",
            sub.title, sub.id, sub.body_md
        ));
    }
    let update = earlier.filter(|_| shared.run(|run| run.kind) == JobKind::Sync);
    let mut system = write::section_system();
    if let Some(earlier) = update {
        message.push_str(&format!(
            "\n\nThe summary before:\n\n{}\n\nThe diagram before:\n\n{}\n",
            earlier.summary_md,
            earlier
                .diagram
                .as_ref()
                .map_or("(none)", |d| d.mermaid.as_str())
        ));
        system.push_str("\n\n");
        system.push_str(write::UPDATE);
    }
    let near: Vec<String> = section
        .subsections
        .iter()
        .flat_map(|sub| sub.files.iter().cloned())
        .collect();
    let ctx = check::Context {
        root: shared.tree,
        elsewhere: page.elsewhere,
        files: page.files,
        index: page.index,
        anchors,
        near: &near,
    };
    let ask = shared.ask_of(
        system,
        message,
        write::schema(true, update.is_some()),
        SECTION,
    );
    let what = format!("the summary of {}", section.id);
    let (written, cost, mut tally) = write_checked(
        shared,
        ask,
        &task,
        &section.title,
        &ctx,
        Room::Part,
        true,
        &what,
    )?;
    if written.unchanged
        && let Some(earlier) = update
    {
        return Ok((
            Summary {
                summary_md: earlier.summary_md.clone(),
                diagram: earlier.diagram.clone(),
                meaning_changed: false,
                cost_usd: cost,
            },
            tally,
        ));
    }
    let (summary, more) = check::link(
        &super::prose::demote_headings(&written.text),
        page.index,
        &near,
    );
    tally += more;
    Ok((
        Summary {
            summary_md: summary,
            diagram: written.diagram,
            meaning_changed: written.meaning_changed,
            cost_usd: cost,
        },
        tally,
    ))
}

/// Writes the overview from the sections' summaries.
fn write_overview(
    shared: &Shared,
    name: &str,
    plan: &Plan,
    sections: &[Section],
    page: &Code,
    anchors: &BTreeSet<String>,
    earlier: Option<&Wiki>,
) -> Result<(Summary, Tally)> {
    let task = format!("Write the overview of the wiki of {name}.");
    let mut message = format!(
        "{task}\n\nWhat the overview is to say, as the outline's planner put it: {}\n\nThe \
         outline of the whole page:\n\n{}\n\nWhere a reader of the code starts: {}\n\nEach \
         section's summary:\n",
        plan.overview,
        plan.outline(),
        plan::entrypoints(page.files).join(", ")
    );
    for section in sections {
        message.push_str(&format!(
            "\n## {} (#{})\n\n{}\n",
            section.title, section.id, section.summary_md
        ));
    }
    let update = earlier.filter(|_| shared.run(|run| run.kind) == JobKind::Sync);
    let mut system = write::overview_system();
    if let Some(earlier) = update {
        message.push_str(&format!(
            "\n\nThe overview before:\n\n{}\n",
            earlier.overview.summary_md
        ));
        system.push_str("\n\n");
        system.push_str(write::UPDATE);
    }
    let ctx = check::Context {
        root: shared.tree,
        elsewhere: page.elsewhere,
        files: page.files,
        index: page.index,
        anchors,
        near: &[],
    };
    let ask = shared.ask_of(
        system,
        message,
        write::schema(true, update.is_some()),
        OVERVIEW,
    );
    let (written, cost, mut tally) = write_checked(
        shared,
        ask,
        &task,
        "Overview",
        &ctx,
        Room::Overview,
        true,
        "the overview",
    )?;
    if written.unchanged
        && let Some(earlier) = update
    {
        return Ok((
            Summary {
                summary_md: earlier.overview.summary_md.clone(),
                diagram: earlier.overview.diagram.clone(),
                meaning_changed: false,
                cost_usd: cost,
            },
            tally,
        ));
    }
    let (summary, more) = check::link(&written.text, page.index, &[]);
    tally += more;
    Ok((
        Summary {
            summary_md: summary,
            diagram: written.diagram,
            meaning_changed: written.meaning_changed,
            cost_usd: cost,
        },
        tally,
    ))
}

/// The sections whose summaries this run writes: every one for a build;
/// for a sync, those whose subsections changed in meaning, came or went,
/// and those the version didn't have.
fn sections_to_write(
    shared: &Shared,
    plan: &Plan,
    sections: &[Section],
    earlier: Option<&Wiki>,
) -> Vec<String> {
    let run = shared.run(Clone::clone);
    sections
        .iter()
        .filter(|section| !run.sections.contains_key(&section.id))
        .filter(|section| {
            if run.kind != JobKind::Sync {
                return true;
            }
            let Some(before) = earlier.and_then(|wiki| wiki.section(&section.id)) else {
                return true;
            };
            let then: Vec<&str> = before.subsections.iter().map(|s| s.id.as_str()).collect();
            let now: Vec<&str> = section.subsections.iter().map(|s| s.id.as_str()).collect();
            let planned = plan.sections.iter().find(|s| s.id == section.id);
            then != now
                || planned.is_some_and(|planned| {
                    planned.subsections.iter().any(|sub| {
                        run.todo.contains(&sub.id)
                            && run
                                .pages
                                .get(&sub.id)
                                .is_some_and(|page| page.meaning_changed)
                    })
                })
        })
        .map(|section| section.id.clone())
        .collect()
}

/// Whether this run writes the overview: a build does; a sync when its
/// outline changed, or a section's summary changed in meaning.
fn overview_to_write(shared: &Shared, plan: &Plan, earlier: Option<&Wiki>) -> bool {
    let run = shared.run(Clone::clone);
    if run.overview.is_some() {
        return false;
    }
    let Some(earlier) = earlier.filter(|_| run.kind == JobKind::Sync) else {
        return true;
    };
    let ids_then: Vec<&str> = earlier.sections.iter().map(|s| s.id.as_str()).collect();
    let ids_now: Vec<&str> = plan.sections.iter().map(|s| s.id.as_str()).collect();
    run.replanned
        || ids_then != ids_now
        || earlier.overview.summary_md.is_empty()
        || run.sections.values().any(|summary| summary.meaning_changed)
}

/// The page's sections with their subsections as this run wrote them, or
/// for a sync, as the version had them, their links moved with their
/// lines; a section's summary as the version had it until it's written.
/// A subsection that couldn't be written is left out.
fn assemble(
    shared: &Shared,
    plan: &Plan,
    earlier: Option<&Wiki>,
    remap: &Remap,
) -> Result<Vec<Section>> {
    let run = shared.run(Clone::clone);
    let mut sections = Vec::new();
    for planned in &plan.sections {
        let mut subsections = Vec::new();
        for sub in &planned.subsections {
            let before = earlier.and_then(|wiki| wiki.subsection(&sub.id));
            let (body_md, diagram) = match (run.pages.get(&sub.id), before) {
                (Some(page), _) => (page.body_md.clone(), page.diagram.clone()),
                // For a sync, what the version had, until it can be
                // written again.
                (None, Some(before)) => (moved(&before.body_md, remap), before.diagram.clone()),
                (None, None) => continue,
            };
            subsections.push(Subsection {
                id: sub.id.clone(),
                title: sub.title.clone(),
                body_md,
                diagram,
                files: sub.files.clone(),
            });
        }
        if subsections.is_empty() {
            continue;
        }
        let (summary_md, diagram) = match earlier.and_then(|wiki| wiki.section(&planned.id)) {
            Some(before) => (moved(&before.summary_md, remap), before.diagram.clone()),
            None => (String::new(), None),
        };
        sections.push(Section {
            id: planned.id.clone(),
            title: planned.title.clone(),
            summary_md,
            diagram,
            subsections,
        });
    }
    if sections.is_empty() {
        bail!("no subsection could be written");
    }
    Ok(sections)
}

/// Every anchor `sections` make on the page, and the overview's.
fn page_anchors(sections: &[Section]) -> BTreeSet<String> {
    let mut anchors: BTreeSet<String> = [plan::OVERVIEW_ID.to_string()].into();
    for section in sections {
        anchors.insert(section.id.clone());
        anchors.extend(section.subsections.iter().map(|sub| sub.id.clone()));
    }
    anchors
}

/// `md` checked one last time against the page as it turned out, what's
/// wrong dropped, and linked; the tally counts only what changed here.
fn final_check(
    md: &str,
    page: &Code,
    tree: &Path,
    anchors: &BTreeSet<String>,
    near: &[String],
) -> (String, Tally) {
    let ctx = check::Context {
        root: tree,
        elsewhere: page.elsewhere,
        files: page.files,
        index: page.index,
        anchors,
        near,
    };
    let checked = check::text(md, &ctx, true);
    let mut tally = Tally {
        moved: checked.tally.moved,
        dropped: checked.tally.dropped,
        ..Tally::default()
    };
    let (linked, more) = check::link(&checked.text, page.index, near);
    tally.linked += more.linked;
    (linked, tally)
}

/// `md`, written at an earlier commit, with its links into the code moved
/// to where their lines are now.
fn moved(md: &str, remap: &Remap) -> String {
    if remap.is_empty() {
        return md.to_string();
    }
    let edits = super::prose::links(md)
        .into_iter()
        .filter_map(|link| {
            let code = CodeLink::parse(&link.dest)?;
            let now = remap.link(&code);
            (now != code).then(|| (link.range, format!("[{}]({})", link.label, now.target())))
        })
        .collect();
    super::prose::splice(md, edits)
}

/// The hash of each file `entries` cover.
fn blobs_of(files: &Files, entries: &[String]) -> BTreeMap<String, String> {
    files
        .covered(entries)
        .into_iter()
        .map(|file| (file.path.clone(), file.blob.clone()))
        .collect()
}

/// Where the repository is on its forge: by the URL it was cloned from, or
/// for one on this machine, its `origin`.
fn web_of(job: &JobSpec) -> Option<repo::Web> {
    let url = match &job.repo.source {
        Source::Git { url } => url.clone(),
        Source::Local { path } => repo::origin(path)?,
    };
    repo::web(&crate::secrets::redact(&url))
}

/// Runs `work` on each of `items`, the settings' concurrency at once, and
/// passes on what the threads report meanwhile; it stops handing out items
/// once the build has stopped or is cancelled.
fn pool<T: Send>(
    shared: &Shared,
    rx: &Receiver<Report>,
    report: &mut dyn FnMut(Report),
    items: Vec<T>,
    work: &(dyn Fn(T) + Sync),
) {
    let workers = shared.config.concurrency.clamp(1, items.len().max(1));
    let queue = Mutex::new(VecDeque::from(items));
    thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    loop {
                        if shared.stopped() || shared.cancel.is_cancelled() {
                            break;
                        }
                        let Some(item) = queue.lock().unwrap().pop_front() else {
                            break;
                        };
                        work(item);
                    }
                })
            })
            .collect();
        loop {
            match rx.recv_timeout(PUMP) {
                Ok(said) => report(said),
                Err(RecvTimeoutError::Timeout) if handles.iter().all(|h| h.is_finished()) => break,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    shared.flush(rx, report);
}

impl Shared<'_> {
    /// Something about the run, read.
    fn run<T>(&self, read: impl FnOnce(&Run) -> T) -> T {
        let state = self.state.lock().unwrap();
        read(state.book.run.as_ref().expect("a run is under way"))
    }

    /// The run changed, and written down.
    fn change<T>(&self, change: impl FnOnce(&mut Run) -> T) -> T {
        let mut state = self.state.lock().unwrap();
        let seconds = state.seconds_before + self.started.elapsed().as_secs();
        let run = state.book.run.as_mut().expect("a run is under way");
        let out = change(run);
        run.seconds = seconds;
        if let Err(err) = state.book.write(&self.book_path) {
            drop(state);
            self.log(&format!("couldn't write the build down: {err:#}"));
        }
        out
    }

    fn save(&self) {
        self.change(|_| ());
    }

    fn stopped(&self) -> bool {
        self.state.lock().unwrap().stopped.is_some()
    }

    fn stopped_why(&self) -> Option<String> {
        self.state.lock().unwrap().stopped.clone()
    }

    /// An error when the build has stopped or was cancelled, saying why
    /// and, as `kept`, what it keeps for a resume.
    fn halt(&self, kept: &str) -> Result<()> {
        if let Some(why) = self.stopped_why() {
            bail!("{why}: {kept}; a resume carries on from there");
        }
        if self.cancel.is_cancelled() {
            bail!("the build was cancelled: {kept}; a resume carries on from there");
        }
        Ok(())
    }

    /// An ask of the settings' model, reading the tree, with `caps`.
    fn ask_of(
        &self,
        system: String,
        message: String,
        schema: serde_json::Value,
        caps: Caps,
    ) -> Ask {
        self.with_caps(
            Ask {
                system,
                tools: Tools::Read,
                schema: Some(schema),
                retries: RETRIES,
                ..Ask::new(&self.config.model, &message)
            },
            caps,
        )
    }

    /// `ask` with `caps`.
    fn with_caps(&self, ask: Ask, caps: Caps) -> Ask {
        Ask {
            max_turns: Some(caps.turns),
            budget_usd: caps.budget_usd,
            timeout: caps.time,
            ..ask
        }
    }

    /// Runs Claude on `ask` within what's left of the budget and the time,
    /// keeps what it cost, and logs it as `what`.
    fn ask(&self, ask: Ask, what: &str) -> Result<claude::Answer> {
        self.preflight
            .get_or_init(|| {
                let checked = claude::preflight(&self.config.model, self.cancel);
                let cost = match &checked {
                    Ok(answer) => answer.cost_usd,
                    Err(failed) => failed.cost_usd,
                };
                self.spend(cost, None);
                match checked {
                    Ok(answer) => {
                        self.log(&format!(
                            "Claude Code read a file with lattice's tools (${:.3})",
                            answer.cost_usd
                        ));
                        Ok(())
                    }
                    Err(failed) => Err(format!("Claude Code couldn't read a file: {failed}")),
                }
            })
            .clone()
            .map_err(|why| anyhow!(why))?;
        let budget = {
            let mut state = self.state.lock().unwrap();
            if let Some(why) = &state.stopped {
                bail!("{why}");
            }
            if self.started.elapsed() >= BUILD_TIME {
                let why = format!(
                    "the build took longer than {}, its most",
                    duration(BUILD_TIME.as_secs())
                );
                state.stopped = Some(why.clone());
                bail!("{why}");
            }
            let left = if self.config.budget_usd > 0.0 {
                self.config.budget_usd - state.spent - state.reserved
            } else {
                f64::INFINITY
            };
            let budget = ask.budget_usd.min(left);
            if budget < LEAST_CALL {
                let why = format!(
                    "the build reached its budget of ${:.2}",
                    self.config.budget_usd
                );
                state.stopped = Some(why.clone());
                bail!("{why}");
            }
            state.reserved += budget;
            budget
        };
        let started = Instant::now();
        let result = claude::run(
            &Ask {
                budget_usd: budget,
                ..ask
            },
            self.tree,
            self.cancel,
            &mut |_| {},
        );
        let cost = match &result {
            Ok(answer) => answer.cost_usd,
            Err(failed) => failed.cost_usd,
        };
        self.state.lock().unwrap().reserved -= budget;
        let model = result.as_ref().ok().and_then(|answer| answer.model.clone());
        self.spend(cost, model);
        match &result {
            Ok(_) => self.log(&format!(
                "{what}: ${cost:.3} in {}s",
                started.elapsed().as_secs()
            )),
            Err(failed) => self.log(&format!(
                "{what} failed after {}s (${cost:.3}): {failed}",
                started.elapsed().as_secs()
            )),
        }
        result.map_err(|failed| anyhow!(failed))
    }

    /// Keeps `cost`, spent by `model`, and tells the job.
    fn spend(&self, cost: f64, model: Option<String>) {
        self.change(|run| {
            run.cost_usd += cost;
            if let Some(model) = model {
                *run.models.entry(model).or_default() += cost;
            }
        });
        let mut state = self.state.lock().unwrap();
        state.spent += cost;
        let total = state.book.run.as_ref().map_or(0.0, |run| run.cost_usd);
        state.progress.cost_usd = round_cents(total);
        let progress = state.progress.clone();
        drop(state);
        let _ = self.tx.send(Report::Progress(progress));
    }

    /// Tells the job the build is at `done` of `total` in `phase`.
    fn progress(&self, phase: Phase, done: u32, total: u32, current: Option<&str>) {
        let mut state = self.state.lock().unwrap();
        if let Some(run) = state.book.run.as_mut() {
            run.phase = phase;
        }
        let cost = state.book.run.as_ref().map_or(0.0, |run| run.cost_usd);
        state.progress = Progress {
            phase,
            done,
            total,
            current: current.map(str::to_string),
            cost_usd: round_cents(cost),
        };
        let progress = state.progress.clone();
        drop(state);
        let _ = self.tx.send(Report::Progress(progress));
    }

    /// Tells the job what the build is writing now.
    fn progress_current(&self, current: &str) {
        let mut state = self.state.lock().unwrap();
        state.progress.current = Some(current.to_string());
        let progress = state.progress.clone();
        drop(state);
        let _ = self.tx.send(Report::Progress(progress));
    }

    fn log(&self, line: &str) {
        let _ = self.tx.send(Report::Log(line.to_string()));
    }

    /// Passes on what's been reported and not yet passed on.
    fn flush(&self, rx: &Receiver<Report>, report: &mut dyn FnMut(Report)) {
        while let Ok(said) = rx.try_recv() {
            report(said);
        }
    }
}

/// A model's name as people say it: `claude-sonnet-5-5` is `Claude Sonnet
/// 5.5`, `claude-haiku-4-5-20251001` `Claude Haiku 4.5`; anything else as
/// it is.
pub fn display_name(model: &str) -> String {
    let Some(rest) = model.strip_prefix("claude-") else {
        return model.to_string();
    };
    let rest = rest.split('[').next().unwrap_or(rest);
    let mut words = Vec::new();
    let mut version = Vec::new();
    for part in rest.split('-') {
        if part.chars().all(|c| c.is_ascii_digit()) {
            // A date at its end isn't its version.
            if part.len() < 8 {
                version.push(part);
            }
        } else {
            let mut chars = part.chars();
            let word: String = chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default();
            words.push(word);
        }
    }
    let mut name = format!("Claude {}", words.join(" "));
    if !version.is_empty() {
        name.push(' ');
        name.push_str(&version.join("."));
    }
    name
}

fn round_cents(usd: f64) -> f64 {
    (usd * 100.0).round() / 100.0
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

fn kind_word(kind: JobKind) -> &'static str {
    match kind {
        JobKind::Sync => "sync",
        _ => "build",
    }
}

fn phase_word(phase: Phase) -> &'static str {
    match phase {
        Phase::Plan => "planning",
        Phase::Write => "writing",
        Phase::Link => "linking",
        Phase::Overview => "the overview",
    }
}

/// `seconds` as a person says it: `42s`, `12m 34s`, `1h 5m`.
fn duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sync_writes_again_what_changed_and_what_it_could_not_write() {
        use crate::r#gen::files::{File, tests::file};
        use crate::r#gen::plan::{Kind, PlannedSection, PlannedSubsection};
        let sub = |id: &str, files: &[&str]| PlannedSubsection {
            id: id.into(),
            title: id.into(),
            about: String::new(),
            kind: Kind::Component,
            files: files.iter().map(|f| f.to_string()).collect(),
        };
        let plan = Plan {
            overview: String::new(),
            sections: vec![PlannedSection {
                id: "s".into(),
                title: "S".into(),
                about: String::new(),
                subsections: vec![
                    sub("queue", &["src/queue/"]),
                    sub("retry", &["src/retry.rs"]),
                    sub("main", &["src/main.rs"]),
                    sub("lost", &["src/lost.rs"]),
                ],
            }],
        };
        let mut files = Files::default();
        let blob = |path: &str, blob: &str| File {
            blob: blob.into(),
            ..file(path)
        };
        files.add(blob("src/queue/push.rs", "new"));
        files.add(blob("src/queue/pop.rs", "same"));
        files.add(blob("src/retry.rs", "same"));
        files.add(blob("src/main.rs", "same"));
        files.add(blob("src/lost.rs", "same"));
        let blobs = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(p, b)| (p.to_string(), b.to_string()))
                .collect()
        };
        let kept = Kept {
            commit: "c1".into(),
            branch: None,
            at: Timestamp(0),
            plan: plan.clone(),
            blobs: [
                (
                    "queue".to_string(),
                    blobs(&[("src/queue/push.rs", "old"), ("src/queue/pop.rs", "same")]),
                ),
                ("retry".to_string(), blobs(&[("src/retry.rs", "same")])),
                ("main".to_string(), blobs(&[("src/main.rs", "same")])),
            ]
            .into(),
            missing: ["lost".to_string()].into(),
            cost_usd: 0.0,
            model: String::new(),
            tally: Tally::default(),
            seconds: 0,
        };
        let page = |id: &str| Subsection {
            id: id.into(),
            title: id.into(),
            body_md: String::new(),
            diagram: None,
            files: Vec::new(),
        };
        let wiki = Wiki {
            version: wiki::VERSION,
            repo: wiki::Repo {
                name: "app".into(),
                root: None,
                commit: "c1".into(),
                branch: None,
                web_url: None,
                code_url: None,
            },
            generated: Generated {
                at: String::new(),
                by: String::new(),
                model: String::new(),
                cost_usd: 0.0,
                lattice: String::new(),
            },
            overview: Overview::default(),
            sections: vec![Section {
                id: "s".into(),
                title: "S".into(),
                summary_md: String::new(),
                diagram: None,
                subsections: vec![page("queue"), page("retry")],
            }],
        };
        // The queue's file changed, `main` isn't in the version, `lost`
        // couldn't be written; `retry` is as it was.
        let changed: Vec<String> = changed(&kept, &wiki, &files).into_iter().collect();
        assert_eq!(changed, ["lost", "main", "queue"]);
        assert!(!replans(2, 10, 3));
        assert!(replans(6, 10, 0), "more than half changed");
        assert!(replans(1, 10, 13), "many new files have no subsection");
        assert!(!replans(0, 0, 0));
    }

    #[test]
    fn a_model_is_named_as_people_say_it() {
        assert_eq!(display_name("claude-sonnet-5-5"), "Claude Sonnet 5.5");
        assert_eq!(
            display_name("claude-haiku-4-5-20251001"),
            "Claude Haiku 4.5"
        );
        assert_eq!(display_name("claude-opus-5-5[1m]"), "Claude Opus 5.5");
        assert_eq!(display_name("sonnet"), "sonnet");
    }

    #[test]
    fn a_duration_reads_as_a_person_says_it() {
        assert_eq!(duration(42), "42s");
        assert_eq!(duration(754), "12m 34s");
        assert_eq!(duration(3900), "1h 5m");
    }

    #[test]
    fn links_written_at_an_earlier_commit_move_with_their_lines() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        repo::tests::run(root, &["init", "-q"]);
        std::fs::write(root.join("a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
        let first = repo::tests::commit(root, "first");
        std::fs::write(
            root.join("a.rs"),
            "// new\n// lines\nfn a() {}\nfn b() {}\n",
        )
        .unwrap();
        let second = repo::tests::commit(root, "second");
        let remap = Remap::between(root, &first, &second).unwrap();
        assert_eq!(
            moved("[`b`](code:a.rs#L2) and [web](https://x.y)", &remap),
            "[`b`](code:a.rs#L4) and [web](https://x.y)"
        );
        assert_eq!(
            moved("[`b`](code:a.rs#L2)", &Remap::default()),
            "[`b`](code:a.rs#L2)"
        );
    }
}
