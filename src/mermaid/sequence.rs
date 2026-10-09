//! `sequenceDiagram` read into its participants and what happens between
//! them, in order: the six message arrows (`->>` a call, `-)` an async
//! message, `-x` a lost one, `->` the open form, each dotted for a reply
//! with a second dash), notes over, left of or right of a lifeline,
//! activations, the `loop`, `alt`, `opt`, `par`, `critical` and `break`
//! frames with their `else`, `and` and `option` dividers, and
//! `autonumber`. A line that's none of these is an error naming it.

use super::flowchart::clean;
use super::graph::Stroke;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub name: String,
    pub actor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    /// `->>`: a filled arrowhead.
    Arrow,
    /// `->`: no head, the line meets the lifeline.
    Open,
    /// `-x`: a cross.
    Cross,
    /// `-)`: an open (async) arrowhead.
    Async,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteAt {
    Left(usize),
    Right(usize),
    Over(usize, usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Message {
        from: usize,
        to: usize,
        text: String,
        stroke: Stroke,
        head: Head,
        /// `+`: the target is activated by it.
        activate: bool,
        /// `-`: the source is deactivated by it.
        deactivate: bool,
    },
    Note {
        at: NoteAt,
        text: String,
    },
    Activate(usize),
    Deactivate(usize),
    /// `loop`, `alt`, `opt`, `par`, `critical`, `break`.
    Open {
        kind: String,
        text: String,
    },
    /// `else`, `and`, `option`.
    Divider {
        text: String,
    },
    Close,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagram {
    pub participants: Vec<Participant>,
    pub events: Vec<Event>,
    pub autonumber: bool,
}

impl Diagram {
    fn participant(&mut self, id: &str) -> usize {
        if let Some(i) = self.participants.iter().position(|p| p.id == id) {
            return i;
        }
        self.participants.push(Participant {
            id: id.to_string(),
            name: id.to_string(),
            actor: false,
        });
        self.participants.len() - 1
    }
}

/// Arrows, longest first so `-->>` is not read as `-->` and a stray `>`.
const ARROWS: &[(&str, Stroke, Head)] = &[
    ("-->>", Stroke::Dotted, Head::Arrow),
    ("->>", Stroke::Solid, Head::Arrow),
    ("--x", Stroke::Dotted, Head::Cross),
    ("-x", Stroke::Solid, Head::Cross),
    ("--)", Stroke::Dotted, Head::Async),
    ("-)", Stroke::Solid, Head::Async),
    ("-->", Stroke::Dotted, Head::Open),
    ("->", Stroke::Solid, Head::Open),
];

const FRAMES: &[&str] = &["loop", "alt", "opt", "par", "critical", "break"];
const DIVIDERS: &[&str] = &["else", "and", "option"];
const IGNORED: &[&str] = &[
    "title",
    "accTitle",
    "accDescr",
    "links",
    "link",
    "properties",
    "details",
    "destroy",
];

pub fn parse(src: &str) -> Result<Diagram, String> {
    let mut lines = super::lines(src);
    lines.next();
    let mut d = Diagram::default();
    // Open blocks: `true` for a frame, `false` for `rect`/`box`.
    let mut open: Vec<bool> = Vec::new();
    for (no, line) in lines {
        let err = |what: &str| format!("line {no}: {what}");
        let (first, rest) = match line.split_once(char::is_whitespace) {
            Some((f, r)) => (f, r.trim()),
            None => (line, ""),
        };
        let lower = first.to_ascii_lowercase();
        match lower.as_str() {
            "participant" | "actor" | "create" => {
                let (actor, rest) = if lower == "create" {
                    match rest.split_once(char::is_whitespace) {
                        Some((kind, r)) => (kind == "actor", r.trim()),
                        None => return Err(err("`create` names no participant")),
                    }
                } else {
                    (lower == "actor", rest)
                };
                let rest = rest.split("@{").next().unwrap_or(rest).trim();
                let (id, name) = match rest.split_once(" as ") {
                    Some((id, name)) => (id.trim(), clean(name).replace('\n', " ")),
                    None => (rest, clean(rest)),
                };
                if id.is_empty() {
                    return Err(err("a participant with no name"));
                }
                let i = d.participant(id);
                d.participants[i].name = name;
                d.participants[i].actor = actor;
                continue;
            }
            "autonumber" => {
                d.autonumber = rest != "off";
                continue;
            }
            "activate" | "deactivate" => {
                if rest.is_empty() {
                    return Err(err("activate names no participant"));
                }
                let i = d.participant(rest);
                d.events.push(if lower == "activate" {
                    Event::Activate(i)
                } else {
                    Event::Deactivate(i)
                });
                continue;
            }
            "note" => {
                let (place, text) = rest
                    .split_once(':')
                    .ok_or_else(|| err("a note needs `: text`"))?;
                let text = clean(text);
                let place = place.trim();
                let lower = place.to_ascii_lowercase();
                let at = if let Some(who) = lower.strip_prefix("left of") {
                    NoteAt::Left(d.participant(place[place.len() - who.len()..].trim()))
                } else if let Some(who) = lower.strip_prefix("right of") {
                    NoteAt::Right(d.participant(place[place.len() - who.len()..].trim()))
                } else if let Some(who) = lower.strip_prefix("over") {
                    let who = &place[place.len() - who.len()..];
                    let mut names = who.split(',').map(str::trim).filter(|s| !s.is_empty());
                    let a = names.next().ok_or_else(|| err("a note over nobody"))?;
                    let a = d.participant(a);
                    let b = match names.next() {
                        Some(b) => d.participant(b),
                        None => a,
                    };
                    NoteAt::Over(a.min(b), a.max(b))
                } else {
                    return Err(err("a note is `left of`, `right of` or `over`"));
                };
                d.events.push(Event::Note { at, text });
                continue;
            }
            "end" if rest.is_empty() => {
                match open.pop() {
                    Some(true) => d.events.push(Event::Close),
                    Some(false) => {}
                    None => return Err(err("`end` without a block")),
                }
                continue;
            }
            "rect" | "box" => {
                open.push(false);
                continue;
            }
            k if FRAMES.contains(&k) => {
                open.push(true);
                d.events.push(Event::Open {
                    kind: k.to_string(),
                    text: clean(rest).replace('\n', " "),
                });
                continue;
            }
            k if DIVIDERS.contains(&k) => {
                if open.last() != Some(&true) {
                    return Err(err("a divider outside a block"));
                }
                d.events.push(Event::Divider {
                    text: clean(rest).replace('\n', " "),
                });
                continue;
            }
            k if IGNORED.contains(&k) => continue,
            _ => {}
        }
        message(&mut d, line).ok_or_else(|| err(&format!("cannot read `{line}`")))?;
    }
    if !open.is_empty() {
        return Err("a block is never closed with `end`".into());
    }
    Ok(d)
}

/// `A->>+B: text`.
fn message(d: &mut Diagram, line: &str) -> Option<()> {
    let bytes = line.as_bytes();
    let mut found = None;
    'scan: for i in 0..bytes.len() {
        if bytes[i] != b'-' {
            continue;
        }
        for (arrow, stroke, head) in ARROWS {
            if line[i..].starts_with(arrow) {
                found = Some((i, arrow.len(), *stroke, *head));
                break 'scan;
            }
        }
    }
    let (at, len, stroke, head) = found?;
    let from = line[..at].trim();
    let rest = &line[at + len..];
    let (target, text) = match rest.split_once(':') {
        Some((t, x)) => (t, clean(x)),
        None => (rest, String::new()),
    };
    let mut target = target.trim();
    let mut activate = false;
    let mut deactivate = false;
    if let Some(t) = target.strip_prefix('+') {
        activate = true;
        target = t.trim();
    } else if let Some(t) = target.strip_prefix('-') {
        deactivate = true;
        target = t.trim();
    }
    if from.is_empty()
        || target.is_empty()
        || target.contains(char::is_whitespace) && target.contains("->")
    {
        return None;
    }
    let from = d.participant(from);
    let to = d.participant(target);
    d.events.push(Event::Message {
        from,
        to,
        text,
        stroke,
        head,
        activate,
        deactivate,
    });
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs(d: &Diagram) -> Vec<(usize, usize, String, Stroke, Head)> {
        d.events
            .iter()
            .filter_map(|e| match e {
                Event::Message {
                    from,
                    to,
                    text,
                    stroke,
                    head,
                    ..
                } => Some((*from, *to, text.clone(), *stroke, *head)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_six_arrows_and_their_strokes() {
        let d = parse(
            "sequenceDiagram\n A->>B: a\n A-->>B: b\n A->B: c\n A-->B: d\n A-xB: e\n A--xB: f\n A-)B: g\n A--)B: h",
        )
        .unwrap();
        use Head::*;
        use Stroke::*;
        let got: Vec<(Stroke, Head)> = msgs(&d).into_iter().map(|m| (m.3, m.4)).collect();
        assert_eq!(
            got,
            vec![
                (Solid, Arrow),
                (Dotted, Arrow),
                (Solid, Open),
                (Dotted, Open),
                (Solid, Cross),
                (Dotted, Cross),
                (Solid, Async),
                (Dotted, Async)
            ]
        );
    }

    #[test]
    fn participants_actors_aliases_and_order_of_first_mention() {
        let d = parse(
            "sequenceDiagram\n actor U as The user\n participant S as Server<br/>side\n C->>S: hi\n U->>C: go",
        )
        .unwrap();
        let names: Vec<(&str, &str, bool)> = d
            .participants
            .iter()
            .map(|p| (p.id.as_str(), p.name.as_str(), p.actor))
            .collect();
        assert_eq!(
            names,
            vec![
                ("U", "The user", true),
                ("S", "Server side", false),
                ("C", "C", false)
            ]
        );
    }

    #[test]
    fn activation_shorthand_notes_and_blocks() {
        let d = parse(
            "sequenceDiagram\n A->>+B: x\n B-->>-A: y\n Note over A,B: both\n note left of A: l\n NOTE RIGHT OF B: r\n loop forever\n  alt ok\n   A->>B: z\n  else no\n  end\n end\n rect rgb(0,0,0)\n  A->>A: me\n end\n autonumber",
        )
        .unwrap();
        assert!(d.autonumber);
        assert!(matches!(d.events[0], Event::Message { activate: true, .. }));
        assert!(matches!(
            d.events[1],
            Event::Message {
                deactivate: true,
                ..
            }
        ));
        assert_eq!(
            d.events[2],
            Event::Note {
                at: NoteAt::Over(0, 1),
                text: "both".into()
            }
        );
        assert!(matches!(
            d.events[3],
            Event::Note {
                at: NoteAt::Left(0),
                ..
            }
        ));
        assert!(matches!(
            d.events[4],
            Event::Note {
                at: NoteAt::Right(1),
                ..
            }
        ));
        let kinds: Vec<&str> = d
            .events
            .iter()
            .map(|e| match e {
                Event::Open { .. } => "open",
                Event::Divider { .. } => "divider",
                Event::Close => "close",
                _ => "-",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "-", "-", "-", "-", "-", "open", "open", "-", "divider", "close", "close", "-"
            ],
            "`rect` is read as nothing"
        );
    }

    #[test]
    fn broken_input_names_its_line() {
        for (src, line) in [
            ("sequenceDiagram\n A->>B: x\n what is this", "line 3"),
            ("sequenceDiagram\n end", "line 2"),
            ("sequenceDiagram\n else", "line 2"),
            ("sequenceDiagram\n Note A: x", "line 2"),
            ("sequenceDiagram\n ->>B: x", "line 2"),
        ] {
            let err = parse(src).unwrap_err();
            assert!(err.starts_with(line), "{src:?}: {err}");
        }
        assert!(parse("sequenceDiagram\n loop x\n A->>B: y").is_err());
    }
}
