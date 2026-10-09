//! What every diagram but the sequence is read into: boxes, the edges
//! between them, and the frames (subgraphs) around them. A flowchart as
//! written, a state diagram's states and transitions, a class or ER
//! diagram's entities and relations.
//!
//! Adapted from crystal's `src/mermaid/graph.rs` (MIT), which lays these
//! out in a terminal; lattice only reads them.

use std::collections::HashMap;

/// How a line is drawn: the three edge styles mermaid has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stroke {
    #[default]
    Solid,
    Dotted,
    Thick,
}

/// Which way the layers run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dir {
    /// `TD` / `TB`: top to bottom.
    #[default]
    Down,
    /// `BT`: bottom to top.
    Up,
    /// `LR`: left to right.
    Right,
    /// `RL`: right to left.
    Left,
}

impl Dir {
    pub fn parse(s: &str) -> Option<Dir> {
        match s.trim().to_ascii_uppercase().as_str() {
            "TD" | "TB" => Some(Dir::Down),
            "BT" => Some(Dir::Up),
            "LR" => Some(Dir::Right),
            "RL" => Some(Dir::Left),
            _ => None,
        }
    }
}

/// One row inside a node's box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Text(String),
    /// A rule across the box: a class's fields from its methods.
    Rule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Rect,
    /// `(( ))`, `( )` and the other rounded forms.
    Round,
    /// `{ }`: a decision.
    Diamond,
    /// A state diagram's `[*]` as a source.
    Start,
    /// …and as a sink.
    End,
    /// A state diagram's fork or join: a thick bar.
    Bar,
}

/// What sits at one end of an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mark {
    #[default]
    None,
    Arrow,
    /// Inheritance and realisation: a hollow triangle.
    Triangle,
    /// Composition: a filled diamond.
    Diamond,
    /// Aggregation: a hollow diamond.
    Hollow,
    Cross,
    Circle,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub rows: Vec<Row>,
    pub shape: Shape,
    /// The innermost subgraph it is in.
    pub group: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    pub stroke: Stroke,
    pub from_mark: Mark,
    pub to_mark: Mark,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub title: String,
    pub parent: Option<usize>,
}

/// A diagram as boxes, edges and frames.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub dir: Dir,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub groups: Vec<Group>,
    index: HashMap<String, usize>,
}

impl Graph {
    pub fn new(dir: Dir) -> Self {
        Self {
            dir,
            ..Self::default()
        }
    }

    /// The node called `id`, made (a box labelled with its id) the first
    /// time it is named.
    pub fn node(&mut self, id: &str) -> usize {
        if let Some(&i) = self.index.get(id) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(Node {
            id: id.to_string(),
            rows: vec![Row::Text(id.to_string())],
            shape: Shape::Rect,
            group: None,
        });
        self.index.insert(id.to_string(), i);
        i
    }

    pub fn find(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    pub fn add_group(&mut self, title: &str, parent: Option<usize>) -> usize {
        self.groups.push(Group {
            title: title.to_string(),
            parent,
        });
        self.groups.len() - 1
    }

    /// Put `node` in `group`, unless it already sits in a subgraph that is
    /// not one of `group`'s ancestors: the first subgraph that names a
    /// node keeps it, a subgraph nested inside that one may take it deeper.
    pub fn assign(&mut self, node: usize, group: Option<usize>) {
        let Some(g) = group else { return };
        match self.nodes[node].group {
            None => self.nodes[node].group = Some(g),
            Some(cur) if cur != g && self.within(Some(g), cur) => self.nodes[node].group = Some(g),
            _ => {}
        }
    }

    pub fn edge(&mut self, from: usize, to: usize, label: Option<String>, stroke: Stroke) {
        self.edges.push(Edge {
            from,
            to,
            label: label.filter(|l| !l.trim().is_empty()),
            stroke,
            from_mark: Mark::None,
            to_mark: Mark::Arrow,
        });
    }

    /// Is cluster `c` (a node's group) `anc` or inside it?
    fn within(&self, mut c: Option<usize>, anc: usize) -> bool {
        while let Some(g) = c {
            if g == anc {
                return true;
            }
            c = self.groups[g].parent;
        }
        false
    }
}
