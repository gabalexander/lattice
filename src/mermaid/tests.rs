//! What [`check`] promises whatever it's given: every diagram under
//! `tests/mermaid/` reads as its kind, other kinds and empty or oversized
//! ones are refused with a reason, and no input makes it panic.

use super::*;
use std::path::Path;

/// Every diagram under `tests/mermaid/`, by name, with its source.
fn fixtures() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mermaid");
    let mut found: Vec<(String, String)> = std::fs::read_dir(dir)
        .expect("the diagrams")
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|x| x == "mmd"))
        .map(|path| {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&path).unwrap())
        })
        .collect();
    found.sort();
    found
}

#[test]
fn every_diagram_reads_as_its_kind() {
    let fixtures = fixtures();
    assert!(fixtures.len() >= 10, "the diagrams are there");
    for (name, source) in fixtures {
        let kind = match name.split('_').next().unwrap() {
            "seq" => Kind::Sequence,
            "flow" => Kind::Flowchart,
            "state" => Kind::State,
            "class" => Kind::Class,
            "er" => Kind::Er,
            other => panic!("{name}: no kind is called {other}"),
        };
        assert_eq!(check(&source), Ok(kind), "{name}");
    }
}

#[test]
fn the_kind_comes_from_the_first_statement() {
    assert_eq!(
        detect("sequenceDiagram\nA->>B: x"),
        Some(Ok(Kind::Sequence))
    );
    assert_eq!(
        detect("%% a comment\n\n  graph LR;A-->B"),
        Some(Ok(Kind::Flowchart))
    );
    assert_eq!(
        detect("---\ntitle: Orders\n---\nerDiagram\n"),
        Some(Ok(Kind::Er))
    );
    assert_eq!(detect("stateDiagram-v2\n"), Some(Ok(Kind::State)));
    assert_eq!(detect("classDiagram\n"), Some(Ok(Kind::Class)));
    assert_eq!(detect("gantt\n title x"), Some(Err("gantt".into())));
    assert_eq!(detect("  \n%% only a comment\n"), None);
}

#[test]
fn other_kinds_and_empty_diagrams_are_refused_and_say_why() {
    for (source, why) in [
        ("pie\n \"a\": 1", "pie diagrams aren't read here"),
        ("gitGraph\n commit", "gitGraph diagrams aren't read here"),
        ("", "the diagram is empty"),
        ("flowchart TD\n", "the diagram has no nodes"),
        ("sequenceDiagram\n", "the diagram has no participants"),
    ] {
        let refused = check(source).unwrap_err();
        assert!(refused.starts_with(why), "{source:?}: {refused}");
    }
    let refused = check("mindmap\n root").unwrap_err();
    assert!(
        refused.contains("write a flowchart, sequenceDiagram"),
        "{refused}"
    );
}

#[test]
fn a_line_that_does_not_read_is_named() {
    let refused = check("flowchart TD\n  a --> b\n  this is not mermaid").unwrap_err();
    assert!(refused.contains("line 3"), "{refused}");
}

#[test]
fn forty_nodes_read_well_and_forty_one_are_refused() {
    let chain = |nodes: usize| {
        let mut source = String::from("flowchart TD\n");
        for i in 0..nodes - 1 {
            source.push_str(&format!(" n{i} --> n{}\n", i + 1));
        }
        source
    };
    assert_eq!(check(&chain(40)), Ok(Kind::Flowchart));
    let refused = check(&chain(41)).unwrap_err();
    assert!(refused.contains("41 nodes"), "{refused}");
    let mut dense = String::from("flowchart TD\n");
    for i in 0..101 {
        dense.push_str(&format!(" a{} --> a{}\n", i % 10, (i + 1) % 10));
    }
    let refused = check(&dense).unwrap_err();
    assert!(refused.contains("101 edges"), "{refused}");
}

/// None may panic. `LATTICE_MERMAID_FUZZ=200000` runs longer.
#[test]
fn the_parsers_never_panic_on_garbage() {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let seeds = fixtures();
    let heads = [
        "flowchart TD\n",
        "graph LR\n",
        "sequenceDiagram\n",
        "stateDiagram-v2\n",
        "classDiagram\n",
        "erDiagram\n",
    ];
    let alphabet: Vec<char> = "AB-->|[](){}:;\"<>x.=&%*\n o|{}~+,日".chars().collect();
    let rounds: usize = std::env::var("LATTICE_MERMAID_FUZZ")
        .ok()
        .and_then(|rounds| rounds.parse().ok())
        .unwrap_or(600);
    for round in 0..rounds {
        let head = heads[(next() as usize) % heads.len()];
        let source: String = match round % 3 {
            0 => {
                let bytes: Vec<u8> = (0..(next() % 120)).map(|_| next() as u8).collect();
                format!("{head}{}", String::from_utf8_lossy(&bytes))
            }
            1 => {
                let mut source = head.to_string();
                for _ in 0..(next() % 100) {
                    source.push(alphabet[(next() as usize) % alphabet.len()]);
                }
                source
            }
            _ => {
                let seed = &seeds[(next() as usize) % seeds.len()].1;
                let mut chars: Vec<char> = seed.chars().collect();
                for _ in 0..(1 + next() % 10) {
                    if chars.is_empty() {
                        break;
                    }
                    let at = (next() as usize) % chars.len();
                    let letter = alphabet[(next() as usize) % alphabet.len()];
                    match next() % 3 {
                        0 => {
                            chars.remove(at);
                        }
                        1 => chars.insert(at, letter),
                        _ => chars[at] = letter,
                    }
                }
                chars.into_iter().collect()
            }
        };
        let _ = check(&source);
    }
}
