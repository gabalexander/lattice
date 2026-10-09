//! The eval: how good a wiki lattice writes is, in numbers, so that a
//! change to the generator ("this should make wikis better") is shown to
//! or not. `docs/evals.md` says how to run it.
//!
//! - `build <repo> <label>`: a golden repository of `evals/config.json`,
//!   cloned at the commit it's pinned to, written by a `lattice` into
//!   `evals/results/<label>/`, with a data directory of its own.
//! - `score <label>`: deterministic metrics (links into the code that land,
//!   core files covered, diagrams that read, size), then the golden
//!   questions of `evals/questions/<repo>.json` answered from the wiki
//!   alone by a model, and the answers graded against their keys.
//! - `pairwise <a> <b>`: two wikis of one repository compared part by
//!   part, each judged twice with the order swapped, a disagreement
//!   counting as a tie: by the topics of the config, or with `--same`, the
//!   subsections of one outline that the two wrote differently.
//! - `report <label>...`: what was scored, side by side.
//!
//! Every model it asks goes through lattice's own runner of Claude Code,
//! with no tools: the judge reads what it's given and nothing else.
//!
//! Adapted from deepwiki-by-cc's eval harness (`evals/`, MIT; see
//! THIRD_PARTY_NOTICES.md).

use anyhow::{Context, Result, bail};
use lattice::cancel::Cancel;
use lattice::claude::{self, Ask};
use lattice::wiki::{CodeLink, Wiki};
use lattice::{r#gen, mermaid, outln};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Duration;

/// The most of a wiki one judge is given to answer from at once: a bigger
/// wiki is given in parts, each answered alone, a section never split.
const PART: usize = 300_000;

/// The most of one page a pairwise judge is given.
const PAGE: usize = 40_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    judge_model: String,
    repos: Vec<Golden>,
}

/// A golden repository, pinned to a commit.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Golden {
    name: String,
    url: String,
    sha: String,
    core_files: Vec<String>,
    pairwise_topics: Vec<Topic>,
}

#[derive(Debug, Clone, Deserialize)]
struct Topic {
    title: String,
    keywords: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Question {
    id: String,
    question: String,
    answer: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// What `build` kept of a run, beside its wiki.
#[derive(Debug, Serialize, Deserialize)]
struct Run {
    label: String,
    repo: String,
    sha: String,
    /// The tree of the commit, which its links point into.
    tree: PathBuf,
    seconds: u64,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            lattice::errln!("eval: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evals");
    let config: Config = serde_json::from_str(&fs::read_to_string(root.join("config.json"))?)
        .context("evals/config.json")?;
    let flag = |name: &str| {
        args.iter()
            .position(|arg| arg == name)
            .and_then(|at| args.get(at + 1))
            .cloned()
    };
    let words: Vec<&String> = {
        let mut words = Vec::new();
        let mut skip = false;
        for arg in args {
            if skip {
                skip = false;
            } else if arg.starts_with("--") {
                skip = arg != "--same";
            } else {
                words.push(arg);
            }
        }
        words
    };
    match words.first().map(|word| word.as_str()) {
        Some("build") if words.len() == 3 => {
            let lattice = flag("--lattice").unwrap_or_else(|| "target/release/lattice".into());
            build(&root, &config, words[1], words[2], &lattice, flag("--from"))
        }
        Some("score") if words.len() == 2 => score(&root, &config, words[1]),
        Some("pairwise") if words.len() == 3 => pairwise(
            &root,
            &config,
            words[1],
            words[2],
            args.iter().any(|arg| arg == "--same"),
        ),
        Some("report") if words.len() > 1 => report(&root, &words[1..]),
        _ => bail!(
            "usage: eval build <repo> <label> [--lattice PATH] [--from CHECKOUT] | score <label> | \
             pairwise <a> <b> [--same] | report <label>..."
        ),
    }
}

fn golden<'a>(config: &'a Config, name: &str) -> Result<&'a Golden> {
    config
        .repos
        .iter()
        .find(|repo| repo.name == name)
        .with_context(|| format!("{name} isn't a golden repository in evals/config.json"))
}

/// `build`: the golden repository `name` at its pin, written by `lattice`
/// with a data directory of its own, its wiki kept as `label`.
fn build(
    root: &Path,
    config: &Config,
    name: &str,
    label: &str,
    lattice: &str,
    from: Option<String>,
) -> Result<()> {
    let golden = golden(config, name)?;
    let data = root.join(".data").join(label);
    let tree = data.join("src").join(name);
    if !tree.join(".git").exists() {
        fs::create_dir_all(tree.parent().expect("a parent"))?;
        let from = from.unwrap_or_else(|| golden.url.clone());
        git(None, &["clone", "--quiet", &from, &tree.to_string_lossy()])?;
    }
    git(
        Some(&tree),
        &["checkout", "--quiet", "-B", "eval", &golden.sha],
    )?;
    git(Some(&tree), &["remote", "set-url", "origin", &golden.url])?;
    let started = std::time::Instant::now();
    let status = Command::new(lattice)
        .args(["build", &tree.to_string_lossy()])
        .env("XDG_CONFIG_HOME", data.join("config"))
        .env("XDG_DATA_HOME", data.join("data"))
        .env("XDG_CACHE_HOME", data.join("cache"))
        .status()
        .with_context(|| format!("couldn't run {lattice}"))?;
    if !status.success() {
        bail!("{lattice} build failed ({status})");
    }
    let repos = data.join("data/lattice/repos");
    let version = fs::read_dir(&repos)?
        .filter_map(|entry| entry.ok())
        .flat_map(|repo| fs::read_dir(repo.path()).into_iter().flatten())
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.join("wiki.json").exists())
        .max_by_key(|path| fs::metadata(path).and_then(|m| m.modified()).ok())
        .context("the build kept no version")?;
    let out = root.join("results").join(label);
    fs::create_dir_all(&out)?;
    for file in ["wiki.json", "build.json", "build.log"] {
        let _ = fs::copy(version.join(file), out.join(file));
    }
    let kept = Run {
        label: label.into(),
        repo: name.into(),
        sha: golden.sha.clone(),
        tree,
        seconds: started.elapsed().as_secs(),
    };
    fs::write(out.join("run.json"), serde_json::to_string_pretty(&kept)?)?;
    outln!("kept {}", out.display())?;
    Ok(())
}

fn git(dir: Option<&Path>, args: &[&str]) -> Result<()> {
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    let out = command.args(args).output().context("couldn't run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// A run kept: its record and its wiki.
fn load(root: &Path, label: &str) -> Result<(Run, Wiki)> {
    let dir = root.join("results").join(label);
    let run: Run = serde_json::from_str(
        &fs::read_to_string(dir.join("run.json"))
            .with_context(|| format!("there's no run called {label}"))?,
    )?;
    let wiki = Wiki::read(&dir.join("wiki.json"))?;
    Ok((run, wiki))
}

/// What the deterministic metrics found.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Metrics {
    sections: usize,
    subsections: usize,
    words: usize,
    diagrams: usize,
    diagrams_read: usize,
    code_links: usize,
    /// Links into a file the commit has, at lines it has.
    links_land: usize,
    /// Links whose label names a symbol, and of those, whose lines have it.
    symbol_links: usize,
    symbol_links_right: usize,
    core_files: usize,
    core_covered: Vec<String>,
    core_missed: Vec<String>,
}

/// The deterministic metrics of `wiki`, its links checked in `tree`.
fn metrics(wiki: &Wiki, tree: &Path, core: &[String]) -> Metrics {
    let mut found = Metrics {
        sections: wiki.sections.len(),
        subsections: wiki.subsections(),
        diagrams: wiki.diagrams(),
        code_links: wiki.code_links(),
        core_files: core.len(),
        ..Metrics::default()
    };
    let mut diagrams: Vec<&str> = Vec::new();
    let mut cards = vec![&wiki.overview.diagram];
    for section in &wiki.sections {
        cards.push(&section.diagram);
        cards.extend(section.subsections.iter().map(|sub| &sub.diagram));
    }
    diagrams.extend(cards.into_iter().flatten().map(|d| d.mermaid.as_str()));
    let texts = wiki.texts();
    let fenced: Vec<String> = texts
        .iter()
        .flat_map(|text| r#gen::fences::mermaid(text))
        .map(|fence| fence.code)
        .collect();
    diagrams.extend(fenced.iter().map(String::as_str));
    found.diagrams_read = diagrams
        .iter()
        .filter(|source| mermaid::check(source).is_ok())
        .count();
    let mut lines: BTreeMap<String, Option<Vec<String>>> = BTreeMap::new();
    let mut read = |path: &str| -> Option<Vec<String>> {
        lines
            .entry(path.to_string())
            .or_insert_with(|| {
                fs::read_to_string(tree.join(path))
                    .ok()
                    .map(|text| text.lines().map(str::to_string).collect())
            })
            .clone()
    };
    for text in &texts {
        found.words += text.split_whitespace().count();
        for link in r#gen::prose::links(text) {
            let Some(code) = CodeLink::parse(&link.dest) else {
                continue;
            };
            let file = read(&code.path);
            let lands = match (&file, code.start) {
                (Some(file), Some(start)) => start >= 1 && start as usize <= file.len(),
                (Some(_), None) => true,
                (None, _) => tree.join(&code.path).is_dir(),
            };
            found.links_land += usize::from(lands);
            if is_citation(&link.label) {
                continue;
            }
            let Some(symbol) = r#gen::check::label_symbol(&link.label) else {
                continue;
            };
            let (Some(file), Some(start)) = (file, code.start) else {
                continue;
            };
            found.symbol_links += 1;
            let word = symbol
                .trim_matches('`')
                .split('(')
                .next()
                .unwrap_or_default()
                .trim_end_matches('!')
                .rsplit([':', '.'])
                .next()
                .unwrap_or_default()
                .to_string();
            let word = word.trim_start_matches("--").replace('-', "_");
            let end = code.end.unwrap_or(start).max(start) as usize;
            let from = (start as usize).saturating_sub(1);
            let near = file.get(from..end.min(file.len())).unwrap_or_default();
            if near.iter().any(|line| line.contains(&word)) {
                found.symbol_links_right += 1;
            }
        }
    }
    let all = texts.join("\n");
    for file in core {
        if all.contains(file.as_str()) {
            found.core_covered.push(file.clone());
        } else {
            found.core_missed.push(file.clone());
        }
    }
    found
}

/// Whether `label` is a citation, `worker.rs:61-118` or `L61-L118`, which
/// names lines, not a symbol.
fn is_citation(label: &str) -> bool {
    let label = label.trim().trim_matches('`');
    let lines = match label.rsplit_once(':') {
        Some((file, lines)) if file.contains(['.', '/']) => lines,
        Some(_) => return false,
        None => match label.strip_prefix('L') {
            Some(lines) => lines,
            None => return false,
        },
    };
    let lines = lines.replace('L', "");
    !lines.is_empty() && lines.split('-').all(|part| part.parse::<u32>().is_ok())
}

/// `md` with each link into the code written as its label and the file it
/// points into, which says where a thing lives in a fraction of the words.
fn plain_links(md: &str) -> String {
    let edits = r#gen::prose::links(md)
        .into_iter()
        .filter_map(|link| {
            let code = CodeLink::parse(&link.dest)?;
            let label = link.label.trim();
            let text = if label.trim_matches('`').contains(&code.path)
                || label.contains(code.path.rsplit('/').next().unwrap_or_default())
            {
                label.to_string()
            } else {
                format!("{label} ({})", code.path)
            };
            Some((link.range, text))
        })
        .collect();
    r#gen::prose::splice(md, edits)
}

/// The wiki as markdown for a judge to answer from, in parts of at most
/// [`PART`] each: its outline by headings, its texts with their links into
/// the code made plain, and what its diagrams show.
fn corpus(wiki: &Wiki) -> Vec<String> {
    // A diagram's caption says what it shows; its source would only take
    // room from the prose.
    let diagram = |out: &mut String, diagram: &Option<lattice::wiki::Diagram>| {
        if let Some(diagram) = diagram {
            out.push_str(&format!("\n(A diagram: {})\n", diagram.caption));
        }
    };
    let mut blocks = Vec::new();
    let mut overview = format!(
        "# {}\n\n{}\n",
        wiki.repo.name,
        plain_links(&wiki.overview.summary_md)
    );
    diagram(&mut overview, &wiki.overview.diagram);
    blocks.push(overview);
    for section in &wiki.sections {
        let mut block = format!(
            "\n## {}\n\n{}\n",
            section.title,
            plain_links(&section.summary_md)
        );
        diagram(&mut block, &section.diagram);
        for sub in &section.subsections {
            block.push_str(&format!(
                "\n### {}\n\n{}\n",
                sub.title,
                plain_links(&sub.body_md)
            ));
            diagram(&mut block, &sub.diagram);
        }
        blocks.push(block);
    }
    let mut parts: Vec<String> = vec![String::new()];
    for block in blocks {
        let last = parts.last_mut().expect("a part");
        if !last.is_empty() && last.len() + block.len() > PART {
            parts.push(block);
        } else {
            last.push_str(&block);
        }
    }
    for part in &mut parts {
        if part.len() > PART {
            let mut at = PART;
            while !part.is_char_boundary(at) {
                at -= 1;
            }
            part.truncate(at);
        }
    }
    parts
}

/// What the judge model answers `message` with, in the shape of `schema`.
fn judge(model: &str, message: String, schema: Value) -> Result<Value> {
    let ask = Ask {
        schema: Some(schema),
        budget_usd: 5.0,
        timeout: Duration::from_secs(20 * 60),
        retries: 2,
        ..Ask::new(model, &message)
    };
    let dir = std::env::temp_dir();
    let answer = claude::run(&ask, &dir, &Cancel::new(), &mut |_| {})
        .map_err(|failed| anyhow::anyhow!("the judge failed: {failed}"))?;
    answer.value.context("the judge gave no structured answer")
}

/// `score`: the run's metrics, and its questions answered and graded.
fn score(root: &Path, config: &Config, label: &str) -> Result<()> {
    let (run, wiki) = load(root, label)?;
    let golden = golden(config, &run.repo)?;
    let metrics = metrics(&wiki, &run.tree, &golden.core_files);
    let questions: Vec<Question> = serde_json::from_str(
        &fs::read_to_string(root.join("questions").join(format!("{}.json", run.repo)))
            .context("its questions")?,
    )?;
    let parts = corpus(&wiki);
    let asked: Vec<Value> = questions
        .iter()
        .map(|q| json!({"id": q.id, "question": q.question}))
        .collect();
    // Each part answered alone; a question's answer is what every part that
    // could answer it said.
    let mut answered: BTreeMap<String, (bool, String)> = BTreeMap::new();
    for (n, part) in parts.iter().enumerate() {
        let whole = if parts.len() == 1 {
            "Below is the complete wiki.".to_string()
        } else {
            format!(
                "Below is part {} of {} of the wiki, whole sections of it; the other parts are \
                 answered from separately.",
                n + 1,
                parts.len()
            )
        };
        let answers = judge(
            &config.judge_model,
            format!(
                "You are evaluating the quality of a generated wiki for a software repository. \
                 {whole} Answer each question using ONLY this wiki: no outside knowledge of the \
                 repository, and no general programming knowledge to fill gaps. If the wiki \
                 doesn't say enough to answer a question, set answerable to false and answer to \
                 an empty string.\n\nBe exhaustive, not summary-level: for each question, search \
                 all of it (facts may be in several places) and include every specific detail it \
                 gives: values, limits, timeouts, retries, orderings, edge cases.\n\n## The \
                 wiki\n\n{part}\n\n## The questions\n\n{}\n\nAnswer every question by its id.",
                serde_json::to_string_pretty(&asked)?
            ),
            json!({"type": "object", "properties": {"answers": {"type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}, "answerable": {"type": "boolean"}, "answer": {"type": "string"}}, "required": ["id", "answerable", "answer"]}}}, "required": ["answers"]}),
        )?;
        for a in answers["answers"].as_array().into_iter().flatten() {
            let id = a["id"].as_str().unwrap_or_default().to_string();
            let entry = answered.entry(id).or_insert((false, String::new()));
            if a["answerable"].as_bool().unwrap_or(false) {
                if !entry.1.is_empty() {
                    entry.1.push_str("\n\n");
                }
                entry.1.push_str(a["answer"].as_str().unwrap_or_default());
                entry.0 = true;
            }
        }
    }
    let to_grade: Vec<Value> = questions
        .iter()
        .filter(|q| answered.get(&q.id).is_some_and(|(ok, _)| *ok))
        .map(|q| {
            json!({"id": q.id, "question": q.question, "expectedAnswer": q.answer, "actualAnswer": answered[&q.id].1})
        })
        .collect();
    let grades = if to_grade.is_empty() {
        json!({"grades": []})
    } else {
        judge(
            &config.judge_model,
            format!(
                "You are grading answers about a codebase against an answer key. For each item, \
                 compare actualAnswer to expectedAnswer: \"correct\" when it matches the key on \
                 every key point, \"partial\" when it gets some key points and misses or gets \
                 others wrong, \"incorrect\" when it contradicts the key or misses its substance. \
                 Judge only factual agreement with the key: wording doesn't matter, and detail \
                 beyond the key is fine unless it contradicts it.\n\n{}\n\nGrade every item by \
                 its id.",
                serde_json::to_string_pretty(&to_grade)?
            ),
            json!({"type": "object", "properties": {"grades": {"type": "array", "items": {"type": "object", "properties": {"id": {"type": "string"}, "verdict": {"type": "string", "enum": ["correct", "partial", "incorrect"]}, "reason": {"type": "string"}}, "required": ["id", "verdict", "reason"]}}}, "required": ["grades"]}),
        )?
    };
    let graded: BTreeMap<String, (String, String)> = grades["grades"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| {
            (
                g["id"].as_str().unwrap_or_default().to_string(),
                (
                    g["verdict"].as_str().unwrap_or("incorrect").to_string(),
                    g["reason"].as_str().unwrap_or_default().to_string(),
                ),
            )
        })
        .collect();
    let items: Vec<Value> = questions
        .iter()
        .map(|q| {
            let (verdict, reason) = match answered.get(&q.id) {
                Some((true, _)) => graded
                    .get(&q.id)
                    .cloned()
                    .unwrap_or_else(|| ("incorrect".into(), "no grade".into())),
                _ => ("unanswerable".into(), "the wiki doesn't say".into()),
            };
            json!({"id": q.id, "tags": q.tags, "verdict": verdict, "reason": reason, "answer": answered.get(&q.id).map(|a| a.1.clone())})
        })
        .collect();
    let part = |tag: Option<&str>| -> Option<f64> {
        let mine: Vec<&Value> = items
            .iter()
            .filter(|item| match tag {
                None => true,
                Some(tag) => item["tags"]
                    .as_array()
                    .is_some_and(|tags| tags.iter().any(|t| t == tag)),
            })
            .collect();
        if mine.is_empty() {
            return None;
        }
        let points: f64 = mine
            .iter()
            .map(|item| match item["verdict"].as_str() {
                Some("correct") => 1.0,
                Some("partial") => 0.5,
                _ => 0.0,
            })
            .sum();
        Some(points / mine.len() as f64)
    };
    let scores = json!({
        "label": label,
        "repo": run.repo,
        "sha": run.sha,
        "judge": config.judge_model,
        "metrics": metrics,
        "qa": {"score": part(None), "floor": part(Some("floor")), "depth": part(Some("depth")), "corpus_parts": parts.len(), "items": items},
    });
    let out = root.join("results").join(label).join("scores.json");
    fs::write(&out, serde_json::to_string_pretty(&scores)?)?;
    print_scores(&scores)?;
    Ok(())
}

fn pct(value: &Value) -> String {
    value
        .as_f64()
        .map_or_else(|| "n/a".into(), |v| format!("{:.0}%", v * 100.0))
}

fn ratio(right: &Value, all: &Value) -> String {
    let (right, all) = (right.as_f64().unwrap_or(0.0), all.as_f64().unwrap_or(0.0));
    if all == 0.0 {
        return "n/a".into();
    }
    format!("{:.0}% ({right}/{all})", right / all * 100.0)
}

fn print_scores(scores: &Value) -> Result<()> {
    let m = &scores["metrics"];
    let qa = &scores["qa"];
    outln!(
        "{} ({} at {})",
        scores["label"],
        scores["repo"],
        scores["sha"]
    )?;
    outln!(
        "  size: {} sections, {} subsections, {} words, {} diagrams, {} links into the code",
        m["sections"],
        m["subsections"],
        m["words"],
        m["diagrams"],
        m["code_links"]
    )?;
    outln!(
        "  links that land: {}",
        ratio(&m["links_land"], &m["code_links"])
    )?;
    outln!(
        "  symbol links on their symbol: {}",
        ratio(&m["symbol_links_right"], &m["symbol_links"])
    )?;
    outln!(
        "  diagrams that read: {}",
        ratio(&m["diagrams_read"], &m["diagrams"])
    )?;
    outln!(
        "  core files covered: {}",
        ratio(
            &json!(m["core_covered"].as_array().map_or(0, Vec::len)),
            &m["core_files"]
        )
    )?;
    outln!(
        "  questions: {} (floor {}, depth {}){}",
        pct(&qa["score"]),
        pct(&qa["floor"]),
        pct(&qa["depth"]),
        if qa["corpus_parts"].as_u64().unwrap_or(1) > 1 {
            ", the wiki answered from in parts"
        } else {
            ""
        }
    )?;
    Ok(())
}

/// The subsection of `wiki` a topic's page is: the one its keywords come up
/// in most, per thousand words, its title counting five times.
fn best_for<'a>(wiki: &'a Wiki, topic: &Topic) -> Option<(&'a str, String)> {
    let mut best: Option<(f64, &str, String)> = None;
    for section in &wiki.sections {
        for sub in &section.subsections {
            let body = sub.body_md.to_lowercase();
            let title = sub.title.to_lowercase();
            let words = body.split_whitespace().count().max(1) as f64;
            let hits: usize = topic
                .keywords
                .iter()
                .map(|kw| {
                    let kw = kw.to_lowercase();
                    title.matches(&kw).count() * 5 + body.matches(&kw).count()
                })
                .sum();
            let score = hits as f64 / words * 1000.0;
            if best.as_ref().is_none_or(|(most, _, _)| score > *most) {
                best = Some((score, &sub.title, sub.body_md.clone()));
            }
        }
    }
    best.map(|(_, title, body)| (title, body))
}

fn clip(text: &str) -> String {
    if text.len() <= PAGE {
        return text.to_string();
    }
    let mut at = PAGE;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    format!("{}\n\n[cut for the comparison]", &text[..at])
}

/// A page compared: its title and its text.
type Page = (String, String);

/// One judgment of two pages on `title`, the first shown as A.
fn judge_pair(model: &str, title: &str, a: &Page, b: &Page) -> Result<(String, String)> {
    let verdict = judge(
        model,
        format!(
            "You are comparing two wiki pages that document the same part of the same \
             repository: \"{title}\". Judge which better helps a developer understand how this \
             part of the system actually works. Weigh, in order:\n1. Behavioural depth: runtime \
             behaviour, data flow, edge cases, failure handling, concrete defaults and limits, \
             rather than an inventory of files and functions.\n2. Grounding: specifics look \
             verifiable (real paths, plausible values); penalize what looks invented or \
             vague.\n3. Connectedness: how this part works with the rest of the system.\n4. \
             Clarity.\nStyle and length alone must not decide. Say tie only if they're genuinely \
             comparable on the above.\n\n## Page A: {}\n\n{}\n\n## Page B: {}\n\n{}\n\nGive your \
             verdict.",
            a.0,
            clip(&a.1),
            b.0,
            clip(&b.1)
        ),
        json!({"type": "object", "properties": {"winner": {"type": "string", "enum": ["A", "B", "tie"]}, "reason": {"type": "string"}}, "required": ["winner", "reason"]}),
    )?;
    Ok((
        verdict["winner"].as_str().unwrap_or("tie").to_string(),
        verdict["reason"].as_str().unwrap_or_default().to_string(),
    ))
}

/// `pairwise`: runs `a` and `b` compared, by topic or with `same`, the
/// subsections one outline gave both that they wrote differently.
fn pairwise(root: &Path, config: &Config, a: &str, b: &str, same: bool) -> Result<()> {
    let (run_a, wiki_a) = load(root, a)?;
    let (run_b, wiki_b) = load(root, b)?;
    if run_a.repo != run_b.repo {
        bail!("{a} and {b} are of different repositories");
    }
    let mut matchups: Vec<(String, Page, Page)> = Vec::new();
    if same {
        for section in &wiki_a.sections {
            for sub in &section.subsections {
                if let Some(other) = wiki_b.subsection(&sub.id)
                    && other.body_md != sub.body_md
                {
                    matchups.push((
                        sub.title.clone(),
                        (sub.title.clone(), sub.body_md.clone()),
                        (other.title.clone(), other.body_md.clone()),
                    ));
                }
            }
        }
    } else {
        let golden = golden(config, &run_a.repo)?;
        for topic in &golden.pairwise_topics {
            let (Some(pa), Some(pb)) = (best_for(&wiki_a, topic), best_for(&wiki_b, topic)) else {
                continue;
            };
            matchups.push((
                topic.title.clone(),
                (pa.0.to_string(), pa.1),
                (pb.0.to_string(), pb.1),
            ));
        }
    }
    let mut results = Vec::new();
    let (mut wins_a, mut wins_b, mut ties) = (0, 0, 0);
    for (title, page_a, page_b) in &matchups {
        let (first, why_first) = judge_pair(&config.judge_model, title, page_a, page_b)?;
        let (second, why_second) = judge_pair(&config.judge_model, title, page_b, page_a)?;
        let one = match first.as_str() {
            "A" => a,
            "B" => b,
            _ => "tie",
        };
        let two = match second.as_str() {
            "A" => b,
            "B" => a,
            _ => "tie",
        };
        let winner = if one == two { one } else { "tie" };
        match winner {
            w if w == a => wins_a += 1,
            w if w == b => wins_b += 1,
            _ => ties += 1,
        }
        outln!(
            "{title}: {winner}{}",
            if one != two { " (split)" } else { "" }
        )?;
        results.push(json!({"title": title, "page_a": page_a.0, "page_b": page_b.0, "verdicts": [one, two], "winner": winner, "reasons": [why_first, why_second]}));
    }
    outln!("{a} {wins_a}, {b} {wins_b}, tie {ties}")?;
    let summary = json!({"a": a, "b": b, "same": same, "judge": config.judge_model, "tally": {a: wins_a, b: wins_b, "tie": ties}, "topics": results});
    let out = root
        .join("results")
        .join(format!("pairwise-{a}-vs-{b}.json"));
    fs::write(&out, serde_json::to_string_pretty(&summary)?)?;
    outln!("{}", out.display())?;
    Ok(())
}

/// `report`: each label's scores.
fn report(root: &Path, labels: &[&String]) -> Result<()> {
    for label in labels {
        let path = root
            .join("results")
            .join(label.as_str())
            .join("scores.json");
        let scores: Value = serde_json::from_str(
            &fs::read_to_string(&path).with_context(|| format!("{label} isn't scored"))?,
        )?;
        print_scores(&scores)?;
    }
    Ok(())
}
