//! `lattice export`: a version of a repository's wiki written out as a
//! static site that works from `file://` and on any static host, GitHub
//! Pages among them. It's the web app's export bundle (`web/build/export/`,
//! carried in the binary: see [`crate::web`]), with mermaid beside its
//! script and the wiki inlined in its page, in the `<script
//! type="application/json" id="wiki-data">` it has for it, since a page
//! opened from a file can't fetch another. The page's code links go to the
//! forge's `code_url`, and its chat says asking needs `lattice serve`.
//!
//! Adapted from crystal's `src/wiki_site.rs` (MIT).

use crate::db::{Db, Repo};
use crate::download::{MERMAID, MERMAID_LICENSE};
use crate::errln;
use crate::paths;
use crate::web;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// The element of the bundle's page that the wiki goes in.
const DATA_TAG: &str = "<script type=\"application/json\" id=\"wiki-data\">";

/// Writes version `version` of `repo`'s wiki, or its latest, as a site in
/// `out`, and gives back its page. Without mermaid, which can't always be
/// downloaded, the diagrams are left as their source, and it says so.
pub fn export(db: &Db, repo: &Repo, version: Option<u32>, out: &Path) -> Result<PathBuf> {
    let versions = db.versions(&repo.key)?;
    let chosen = match version {
        Some(n) => versions.iter().find(|v| v.n == n),
        None => versions.last(),
    };
    let Some(chosen) = chosen else {
        match version {
            Some(n) => bail!("{} has no version {n}", repo.name),
            None => bail!(
                "{} has no wiki yet: `lattice build {}` writes one",
                repo.name,
                repo.key
            ),
        }
    };
    let source = paths::version_dir(&repo.key, chosen.n).join(paths::WIKI_FILE);
    let wiki = fs::read_to_string(&source)
        .with_context(|| format!("couldn't read {}", source.display()))?;
    serde_json::from_str::<serde_json::Value>(&wiki)
        .with_context(|| format!("{} isn't JSON", source.display()))?;
    let mut files = web::export_files().peekable();
    if files.peek().is_none() {
        bail!(
            "this lattice was built without its web app, which the site is made of: \
             `make web`, then build lattice again"
        );
    }
    let mermaid = match MERMAID.fetch() {
        Ok(mermaid) => Some(mermaid),
        Err(err) => {
            errln!("the diagrams are left as their source: {err:#}");
            None
        }
    };
    write_site(files, &wiki, mermaid.as_deref(), out)?;
    Ok(out.join("index.html"))
}

/// Writes the site in `out`: `files`, the bundle, its `index.html` with
/// `wiki` inlined, `wiki.json` beside it, and `mermaid`, with its license,
/// beside the bundle's `wiki.js`, when there is one.
fn write_site<'a>(
    files: impl Iterator<Item = (&'a str, &'a [u8])>,
    wiki: &str,
    mermaid: Option<&Path>,
    out: &Path,
) -> Result<()> {
    let write = |path: PathBuf, bytes: &[u8]| -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("couldn't make {}", dir.display()))?;
        }
        fs::write(&path, bytes).with_context(|| format!("couldn't write {}", path.display()))
    };
    let mut scripts = PathBuf::new();
    for (name, bytes) in files {
        if name == "index.html" {
            let page = String::from_utf8_lossy(bytes);
            write(out.join(name), inlined(&page, wiki).as_bytes())?;
            continue;
        }
        if Path::new(name)
            .file_name()
            .is_some_and(|file| file == "wiki.js")
        {
            scripts = Path::new(name)
                .parent()
                .unwrap_or(Path::new(""))
                .to_path_buf();
        }
        write(out.join(name), bytes)?;
    }
    write(out.join(paths::WIKI_FILE), wiki.as_bytes())?;
    // GitHub Pages leaves out files whose names start with `_` otherwise.
    write(out.join(".nojekyll"), b"")?;
    if let Some(mermaid) = mermaid {
        let to = out.join(&scripts).join(MERMAID.name);
        fs::copy(mermaid, &to).with_context(|| format!("couldn't write {}", to.display()))?;
        write(
            out.join(&scripts).join("mermaid.LICENSE.txt"),
            MERMAID_LICENSE.as_bytes(),
        )?;
    }
    Ok(())
}

/// `page` with `wiki`, the text of a `wiki.json`, in its `wiki-data`
/// element, or in one of its own before `</head>` when it has none; every
/// `<` in it written `<`, so nothing in the wiki's text can end the
/// element. JSON's strings are the only place a `<` can be.
pub fn inlined(page: &str, wiki: &str) -> String {
    let data = wiki.trim_end().replace('<', "\\u003c");
    let empty = format!("{DATA_TAG}</script>");
    if page.contains(&empty) {
        return page.replacen(&empty, &format!("{DATA_TAG}{data}</script>"), 1);
    }
    let tag = format!("{DATA_TAG}{data}</script>\n");
    match page.find("</head>") {
        Some(at) => format!("{}{tag}{}", &page[..at], &page[at..]),
        None => format!("{tag}{page}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wiki's data as the page would read it from `page`.
    fn data_in(page: &str) -> serde_json::Value {
        let start = page.find(DATA_TAG).unwrap() + DATA_TAG.len();
        let end = page[start..].find("</script>").unwrap();
        serde_json::from_str(&page[start..start + end]).unwrap()
    }

    #[test]
    fn the_wiki_fills_its_element_and_can_t_end_it() {
        let wiki = "{\"t\":\"</script><script>alert(1)</script>\"}\n";
        let page = format!("<html><head>{DATA_TAG}</script></head><body></body></html>");
        let filled = inlined(&page, wiki);
        assert_eq!(filled.matches(DATA_TAG).count(), 1);
        assert!(!filled.contains("<script>alert"), "{filled}");
        assert_eq!(data_in(&filled)["t"], "</script><script>alert(1)</script>");
        let bare = "<html><head><title>w</title></head><body></body></html>";
        let added = inlined(bare, wiki);
        assert!(added.find(DATA_TAG).unwrap() < added.find("</head>").unwrap());
        assert_eq!(data_in(&added)["t"], "</script><script>alert(1)</script>");
    }

    #[test]
    fn the_site_is_the_bundle_with_the_wiki_and_mermaid_beside_its_script() {
        let dir = tempfile::tempdir().unwrap();
        let mermaid = dir.path().join("mermaid.min.js");
        fs::write(&mermaid, "window.mermaid = {};").unwrap();
        let page = format!("<!doctype html><head>{DATA_TAG}</script></head>");
        let files: Vec<(&str, &[u8])> = vec![
            ("index.html", page.as_bytes()),
            ("assets/wiki.js", b"render()"),
            ("assets/wiki.css", b"body{}"),
            ("fonts/a.woff2", b"font"),
        ];
        let out = dir.path().join("site");
        let wiki = r#"{"version":1,"repo":{"name":"acme/app"}}"#;
        write_site(files.into_iter(), wiki, Some(&mermaid), &out).unwrap();
        let index = fs::read_to_string(out.join("index.html")).unwrap();
        assert_eq!(data_in(&index)["repo"]["name"], "acme/app");
        assert_eq!(fs::read_to_string(out.join("wiki.json")).unwrap(), wiki);
        assert_eq!(
            fs::read_to_string(out.join("assets/mermaid.min.js")).unwrap(),
            "window.mermaid = {};"
        );
        let license = fs::read_to_string(out.join("assets/mermaid.LICENSE.txt")).unwrap();
        assert!(license.contains("Copyright (c) 2014 - 2022 Knut Sveidqvist"));
        for file in [
            "assets/wiki.js",
            "assets/wiki.css",
            "fonts/a.woff2",
            ".nojekyll",
        ] {
            assert!(out.join(file).is_file(), "{file}");
        }
    }
}
