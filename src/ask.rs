//! The chat: a question about a repository's code, asked on its wiki's
//! page, answered by Claude Code in the repository, its answer streamed
//! back to the page as it comes, as server-sent events: `delta`, the
//! answer's text as it's written; `tool`, a tool it uses and the file it
//! reads; then `done`, with its conversation and what it cost, or `error`,
//! saying why it failed.
//!
//! One run a question (see [`crate::claude`]), locked down and reading
//! alone, with the settings' `ask_model` and `ask_budget_usd`, given the
//! question on its standard input and, appended to its system prompt, the
//! wiki's outline (each section's title and summary, and the section the
//! user is reading in full) and the rule to answer with `code:` links to
//! the lines it read. A follow-up resumes the conversation the page was
//! told of. A page that goes away stops its run.
//!
//! Adapted from crystal's `src/wiki_ask.rs` (MIT).

use crate::cancel::Cancel;
use crate::claude::{self, Ask, Conversation, Event, Reason, Tools};
use crate::config::Config;
use crate::http;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

/// The longest question taken, in bytes.
pub const MAX_QUESTION: usize = 8 * 1024;

/// The most of the section the user is reading that Claude is given, in
/// bytes: the rest it reads in the code.
const SECTION_CAP: usize = 24 * 1024;

/// The longest a question may take.
const TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// What the page asks: `POST /api/repos/<key>/ask`'s body.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Question {
    pub question: String,
    /// The conversation an earlier answer named, to follow up in.
    #[serde(default)]
    pub conversation: Option<String>,
    /// The id of the section the user is reading.
    #[serde(default)]
    pub section: Option<String>,
}

impl Question {
    /// Why the question can't be asked, if it can't.
    pub fn refused(&self) -> Option<String> {
        if self.question.trim().is_empty() {
            return Some("the question is empty".to_string());
        }
        if self.question.len() > MAX_QUESTION {
            return Some(format!(
                "the question is longer than {} KiB",
                MAX_QUESTION / 1024
            ));
        }
        if let Some(conversation) = &self.conversation
            && !claude::is_conversation(conversation)
        {
            return Some(format!(
                "{} isn't a conversation",
                crate::printable::line(conversation)
            ));
        }
        None
    }
}

/// Answers `question` about the repository whose wiki is `wiki`, its code
/// in `root`, with `config`'s model and budget, writing the events the
/// page is told to `out` as they come. Fails only when the page has gone,
/// once Claude is stopped.
pub fn answer(
    question: &Question,
    wiki: &Value,
    root: &Path,
    config: &Config,
    out: &mut dyn Write,
) -> io::Result<()> {
    if let Some(why) = question.refused() {
        return http::event(out, "error", &json!({ "message": why }));
    }
    let ask = Ask {
        system: system_prompt(wiki, question.section.as_deref()),
        tools: Tools::Read,
        budget_usd: config.ask_budget_usd,
        conversation: match &question.conversation {
            Some(id) => Conversation::Resume(id.clone()),
            None => Conversation::Keep,
        },
        stream: true,
        timeout: TIMEOUT,
        ..Ask::new(&config.ask_model, &question.question)
    };
    let cancel = Cancel::new();
    let mut gone = None;
    let answered = claude::run(&ask, root, &cancel, &mut |event| {
        if gone.is_some() {
            return;
        }
        let told = match event {
            Event::Text(text) => http::event(out, "delta", &json!({ "text": text })),
            Event::Tool { name, path, detail } => {
                let mut data = json!({ "name": name });
                if let Some(path) = path {
                    data["path"] = json!(path);
                }
                // It reads alone: what else a tool is asked is a pattern.
                if let Some(pattern) = detail {
                    data["pattern"] = json!(pattern);
                }
                http::event(out, "tool", &data)
            }
            Event::Quiet => http::keep_alive(out),
            Event::Started { .. } => Ok(()),
        };
        if let Err(err) = told {
            // The page has gone: so has what it asked.
            gone = Some(err);
            cancel.cancel();
        }
    });
    if let Some(err) = gone {
        return Err(err);
    }
    match answered {
        Ok(answer) => http::event(
            out,
            "done",
            &json!({ "conversation": answer.conversation, "cost_usd": answer.cost_usd }),
        ),
        Err(failed) => {
            let message = match failed.reason {
                Reason::Budget => format!(
                    "answering would cost more than the ${:.2} a question may spend: \
                     ask_budget_usd in the settings says how much",
                    config.ask_budget_usd
                ),
                _ => failed.why,
            };
            http::event(out, "error", &json!({ "message": message }))
        }
    }
}

/// What Claude is told beside its own system prompt: what it's answering
/// about, how to link into the code, and the wiki's outline, the section
/// with the id `reading` in full.
pub fn system_prompt(wiki: &Value, reading: Option<&str>) -> String {
    let repo = &wiki["repo"];
    let name = repo["name"].as_str().unwrap_or("this project");
    let commit = repo["commit"].as_str().unwrap_or("unknown");
    let mut prompt = format!(
        "You answer questions about the code of {name}, asked by someone reading its wiki, a \
         page about its code. You are in its repository; the wiki was written at commit \
         {commit}, and the code may have moved on since. Read the code to answer: never answer \
         from the outline below alone, and never make up a line you haven't read.\n\n\
         Answer in markdown, briefly and exactly. Whenever you name a file, a type, a function \
         or a line, link it into the code: [label](code:PATH#L10) for a line, \
         [label](code:PATH#L10-L20) for lines, [label](code:PATH) for a whole file, PATH from \
         the repository's root, the lines as you read them. Link to a part of the wiki with \
         [title](#id), by the ids below.\n\nThe wiki's outline:\n"
    );
    let summary = |value: &Value| first_paragraph(value.as_str().unwrap_or_default());
    prompt.push_str(&format!(
        "- Overview: {}\n",
        summary(&wiki["overview"]["summary_md"])
    ));
    let sections = wiki["sections"].as_array().map(Vec::as_slice);
    let mut reading_in = None;
    for section in sections.unwrap_or_default() {
        let id = section["id"].as_str().unwrap_or_default();
        prompt.push_str(&format!(
            "- {} (#{id}): {}\n",
            section["title"].as_str().unwrap_or_default(),
            summary(&section["summary_md"]),
        ));
        let subsections = section["subsections"].as_array().map(Vec::as_slice);
        for subsection in subsections.unwrap_or_default() {
            let sub = subsection["id"].as_str().unwrap_or_default();
            prompt.push_str(&format!(
                "  - {} (#{sub})\n",
                subsection["title"].as_str().unwrap_or_default()
            ));
            if reading == Some(sub) {
                reading_in = Some((section, Some(subsection)));
            }
        }
        if reading == Some(id) {
            reading_in = Some((section, None));
        }
    }
    if let Some((section, subsection)) = reading_in {
        let mut text = String::new();
        text.push_str(&format!(
            "## {}\n\n{}\n\n",
            section["title"].as_str().unwrap_or_default(),
            section["summary_md"].as_str().unwrap_or_default()
        ));
        for sub in section["subsections"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            text.push_str(&format!(
                "### {}\n\n{}\n\n",
                sub["title"].as_str().unwrap_or_default(),
                sub["body_md"].as_str().unwrap_or_default()
            ));
        }
        let at = match subsection {
            Some(sub) => format!(
                "\"{}\", under \"{}\"",
                sub["title"].as_str().unwrap_or_default(),
                section["title"].as_str().unwrap_or_default()
            ),
            None => format!("\"{}\"", section["title"].as_str().unwrap_or_default()),
        };
        prompt.push_str(&format!(
            "\nThe user is reading {at}. Its section of the wiki, in full:\n\n{}",
            cut(&text, SECTION_CAP)
        ));
    }
    prompt
}

/// The first paragraph of `markdown`, on one line.
fn first_paragraph(markdown: &str) -> String {
    let paragraph = markdown.trim().split("\n\n").next().unwrap_or_default();
    paragraph.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text`, cut to at most `cap` bytes at a character's edge.
fn cut(text: &str, cap: usize) -> &str {
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_refused_empty_too_long_or_in_a_conversation_that_isn_t_one() {
        let question = |text: &str, conversation: Option<&str>| Question {
            question: text.into(),
            conversation: conversation.map(String::from),
            section: None,
        };
        assert_eq!(question("How?", None).refused(), None);
        let uuid = "ebf4cee3-7c34-4945-a896-201b1a2fbfc4";
        assert_eq!(question("How?", Some(uuid)).refused(), None);
        assert!(question("  \n", None).refused().is_some());
        assert!(
            question(&"x".repeat(MAX_QUESTION + 1), None)
                .refused()
                .is_some()
        );
        assert!(
            question("How?", Some("--dangerously-skip-permissions"))
                .refused()
                .is_some()
        );
    }

    #[test]
    fn claude_is_told_the_outline_and_the_section_being_read_in_full() {
        let wiki = json!({
            "repo": {"name": "acme/app", "commit": "abc123"},
            "overview": {"summary_md": "An app.\n\nMore about it."},
            "sections": [
                {"id": "start", "title": "Starting", "summary_md": "How it starts.",
                 "subsections": [{"id": "main", "title": "Main", "body_md": "The main body."}]},
                {"id": "end", "title": "Ending", "summary_md": "How it ends.",
                 "subsections": [{"id": "exit", "title": "Exit", "body_md": "The exit body."}]}
            ]
        });
        let prompt = system_prompt(&wiki, Some("main"));
        assert!(prompt.contains("the code of acme/app"));
        assert!(prompt.contains("at commit abc123"));
        assert!(prompt.contains("[label](code:PATH#L10-L20)"));
        assert!(prompt.contains("- Overview: An app.\n"), "{prompt}");
        assert!(prompt.contains("- Starting (#start): How it starts.\n  - Main (#main)\n"));
        assert!(prompt.contains("- Ending (#end): How it ends.\n  - Exit (#exit)\n"));
        assert!(prompt.contains("The user is reading \"Main\", under \"Starting\""));
        assert!(prompt.contains("### Main\n\nThe main body."));
        assert!(!prompt.contains("The exit body."), "only the section read");
        let prompt = system_prompt(&wiki, None);
        assert!(!prompt.contains("The user is reading"));
        assert!(!prompt.contains("The main body."));
    }
}
