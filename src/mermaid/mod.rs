//! Reading mermaid diagrams, to check one a model wrote before it goes on
//! a wiki's page: a diagram the browser can't draw shows as an error box
//! where the picture should be, so one that doesn't read here is sent back
//! to be written again.
//!
//! What's read is the common subset of the five kinds a model explains
//! code with, and anything past it is refused with a reason, which the
//! model is told:
//!
//! * `sequenceDiagram`: participants, the six message arrows, notes,
//!   activations, `loop`/`alt`/`opt`/`par`/`critical`/`break` frames and
//!   `autonumber` ([`sequence`]).
//! * `flowchart` / `graph`: every direction, the node shapes, the four edge
//!   strokes, labels, `&`, chains and subgraphs.
//! * `stateDiagram-v2`: start and end, described and composite states,
//!   choice, fork and join.
//! * `classDiagram` and `erDiagram`: entities with their members, and
//!   relations with their cardinalities.
//!
//! All but the sequence are read into one [`graph::Graph`]. It's all pure
//! functions that never panic, whatever they're given (the tests throw
//! garbage at the parsers): the diagrams come from what a model wrote.
//!
//! Adapted from crystal's `src/mermaid/` (MIT), itself from docket's
//! `docket-mermaid` crate, without the drawing in a terminal.

mod class;
mod er;
mod flowchart;
pub mod graph;
pub mod sequence;
mod state;

use graph::Graph;

/// The most boxes a diagram may have: past this it's a wall of boxes
/// nobody reads, and the page is better with two diagrams, or one about
/// less.
pub const MAX_NODES: usize = 40;

/// The most edges, for the same reason.
pub const MAX_EDGES: usize = 100;

/// Which kind of diagram it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Sequence,
    Flowchart,
    State,
    Class,
    Er,
}

/// A diagram as it was read.
#[derive(Debug, Clone)]
pub enum Diagram {
    Sequence(sequence::Diagram),
    Graph { kind: Kind, graph: Graph },
}

impl Diagram {
    pub fn kind(&self) -> Kind {
        match self {
            Diagram::Sequence(_) => Kind::Sequence,
            Diagram::Graph { kind, .. } => *kind,
        }
    }
}

/// `source` read, or why it couldn't be: a kind not read here, or the
/// line that doesn't read, by its number.
pub fn parse(source: &str) -> Result<Diagram, String> {
    let kind = match detect(source) {
        None => return Err("the diagram is empty".into()),
        Some(Err(name)) => {
            return Err(format!(
                "{name} diagrams aren't read here: write a flowchart, sequenceDiagram, \
                 stateDiagram-v2, classDiagram or erDiagram"
            ));
        }
        Some(Ok(kind)) => kind,
    };
    let graph = match kind {
        Kind::Sequence => return sequence::parse(source).map(Diagram::Sequence),
        Kind::Flowchart => flowchart::parse(source)?,
        Kind::State => state::parse(source)?,
        Kind::Class => class::parse(source)?,
        Kind::Er => er::parse(source)?,
    };
    Ok(Diagram::Graph { kind, graph })
}

/// Whether `source` is a diagram fit for a page, and its kind; or why it
/// isn't: it doesn't read, it's empty, or it's too big to read well.
pub fn check(source: &str) -> Result<Kind, String> {
    match parse(source)? {
        Diagram::Sequence(diagram) if diagram.participants.is_empty() => {
            Err("the diagram has no participants".into())
        }
        Diagram::Sequence(_) => Ok(Kind::Sequence),
        Diagram::Graph { graph, .. } if graph.nodes.is_empty() => {
            Err("the diagram has no nodes".into())
        }
        Diagram::Graph { graph, .. } if graph.nodes.len() > MAX_NODES => Err(format!(
            "{} nodes: at most {MAX_NODES} read well",
            graph.nodes.len()
        )),
        Diagram::Graph { graph, .. } if graph.edges.len() > MAX_EDGES => Err(format!(
            "{} edges: at most {MAX_EDGES} read well",
            graph.edges.len()
        )),
        Diagram::Graph { kind, .. } => Ok(kind),
    }
}

/// The diagram's kind, from its first statement: `Some(Ok(kind))` for one
/// read here, `Some(Err(name))` for one that isn't, `None` for an empty
/// source.
pub fn detect(source: &str) -> Option<Result<Kind, String>> {
    let (_, first) = lines(source).next()?;
    let word = first
        .split(|c: char| c.is_whitespace() || c == ';')
        .next()
        .unwrap_or("");
    Some(match word {
        "sequenceDiagram" => Ok(Kind::Sequence),
        "flowchart" | "graph" | "flowchart-elk" => Ok(Kind::Flowchart),
        "stateDiagram" | "stateDiagram-v2" => Ok(Kind::State),
        "classDiagram" | "classDiagram-v2" => Ok(Kind::Class),
        "erDiagram" => Ok(Kind::Er),
        "" => Err("unnamed".to_string()),
        other => Err(other.to_string()),
    })
}

/// The source's statements, each with its line number and trimmed: blank
/// lines, `%%` comments, `%%{init}%%` directives and a `---` front matter
/// block (a diagram's title and config) left out.
fn lines(source: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut in_front_matter = false;
    let mut seen = false;
    source.lines().enumerate().filter_map(move |(index, raw)| {
        let line = raw.trim();
        if line == "---" && (!seen || in_front_matter) {
            in_front_matter = !in_front_matter;
            seen = true;
            return None;
        }
        if in_front_matter || line.is_empty() || line.starts_with("%%") {
            return None;
        }
        seen = true;
        Some((index + 1, line))
    })
}

#[cfg(test)]
mod tests;
