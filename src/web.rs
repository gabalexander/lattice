//! The web app, as the binary carries it: the files `npm --prefix web run
//! build` wrote under `web/build/` when lattice was built (see `build.rs`),
//! served by the server as a single-page app, and the bundle under
//! `export/` that `lattice export` writes a static site from. A lattice
//! built before the app was has none, and says how to build it.

include!(concat!(env!("OUT_DIR"), "/web_files.rs"));

/// The page shown when lattice was built without its web app.
const NOT_BUILT: &str = "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
     <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
     <title>lattice</title><style>body{margin:0;padding:24px;background:#000;color:#fff;\
     font:15px/1.6 system-ui,sans-serif}main{max-width:640px;margin:0 auto}code{background:#36373a;\
     border-radius:4px;padding:2px 6px}</style></head><body><main><h1>lattice</h1>\
     <p>This lattice was built without its web app. Build the app, then lattice again:</p>\
     <p><code>make web &amp;&amp; cargo build</code></p>\
     <p>The server's API answers meanwhile, under <code>/api/</code>.</p></main></body></html>\n";

/// The file of the app at `path`, from `web/build/`.
pub fn file(path: &str) -> Option<&'static [u8]> {
    FILES
        .binary_search_by(|(name, _)| (*name).cmp(path))
        .ok()
        .map(|at| FILES[at].1)
}

/// The app's page, which every address of its own is answered with, or
/// the page saying it wasn't built.
pub fn index() -> &'static [u8] {
    file("index.html").unwrap_or(NOT_BUILT.as_bytes())
}

/// Whether the app was built into this lattice.
pub fn is_built() -> bool {
    file("index.html").is_some()
}

/// The files of the static site's bundle, by their paths in it.
pub fn export_files() -> impl Iterator<Item = (&'static str, &'static [u8])> {
    FILES
        .iter()
        .filter_map(|(name, bytes)| Some((name.strip_prefix("export/")?, *bytes)))
}

/// Whether the file at `path` never changes under its name: the app's
/// hashed files.
pub fn is_immutable(path: &str) -> bool {
    path.starts_with("_app/immutable/")
}

/// What a file is, by its name, for a `Content-Type`.
pub fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("webmanifest") => "application/manifest+json",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("txt" | "log") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_files_are_in_order_and_found_by_their_paths() {
        assert!(FILES.windows(2).all(|pair| pair[0].0 < pair[1].0));
        for (name, bytes) in FILES {
            assert_eq!(file(name), Some(*bytes));
        }
        assert_eq!(file("../Cargo.toml"), None);
        assert!(!index().is_empty());
        if !is_built() {
            let page = String::from_utf8_lossy(index());
            assert!(page.contains("built without its web app"), "{page}");
        }
    }

    #[test]
    fn a_file_s_type_is_by_its_extension() {
        assert_eq!(content_type("app.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            content_type("_app/immutable/x.css"),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type("fonts/a.woff2"), "font/woff2");
        assert_eq!(content_type("LICENSE"), "application/octet-stream");
        assert!(is_immutable("_app/immutable/entry/start.js"));
        assert!(!is_immutable("index.html"));
    }
}
