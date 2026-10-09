# The web app

`lattice serve` and `lattice open` show a web app on 127.0.0.1: the wikis made so far, a box to make another,
each job as it runs, each wiki as a page like Google's Code Wiki, with a chat that reads the code to answer, and
the settings. It lives in `web/`: a SvelteKit app built to static files, which the release binary carries, so
running lattice needs no Node.

## Its pages

| Path | What it shows |
| --- | --- |
| `/` | A box taking a folder, a git URL or GitHub's `owner/repo`, the model (Sonnet or Opus) and Generate; below it every wiki as a card: its name, where it comes from, its latest version's commit, age, model and cost, and while a job runs for it, its progress, live. |
| `/jobs/<id>` | A job: its phase (plan, write, link, overview), how far along, the subsection it's writing, what it has cost, the subsections written, its log, and Cancel. When it's done it moves on to the version it made. |
| `/<key>` | A repo's latest wiki: the outline on the left following the reader, the document in the middle (the overview beside its diagram, then each section and subsection with its diagram card, zoomable), links into the code, and the chat on the right. Above the document, the version shown (every build is kept, with its model), Sync, Resume and Regenerate, and a banner for a job running, a version made since, or code that has moved on. |
| `/<key>/v/<n>` | The same, at version `n`. |
| `/settings` | The settings `docs/configuration.md` lists. |

A repo with no version yet shows its build, or a button to start one. Keys never take the app's own names
(`settings`, `jobs`, `api`, `assets`, `_app`, `fonts`, `export`).

On a wiki's page, `/` finds a section or another wiki, `c` shows or hides the chat, `j` and `k` go to the next
or previous section, and Esc closes what's open. A name in code opens the file at its line in `$VISUAL` or
`$EDITOR`; with ⌘ or Ctrl it opens on the forge, at the commit the wiki was written from.

## What it asks lattice

Everything is under `/api/`, as JSON, but for the two streams. An error is a status with `{"message"}`, which
the app shows as it is.

| Request | Answer |
| --- | --- |
| `GET /api/repos` | `[{key, name, source: {kind: "local", path} or {kind: "git", url}, versions: [{n, commit, branch, model, at, cost_usd}], job}]` |
| `POST /api/repos` `{source}` | `{key}`: the repo, added or found (a git source is cloned into lattice's data directory by its first job) |
| `DELETE /api/repos/<key>` | 204 |
| `POST /api/repos/<key>/jobs` `{kind, model, concurrency}` | the job: `kind` is `build`, `sync`, `resume` or `regenerate`; `model` and `concurrency` null for the settings' |
| `GET /api/jobs/<id>` | the job: `{id, repo, kind, state, model, concurrency, created, progress, started, finished, error, version}`, `state` one of `queued`, `running`, `done`, `failed`, `cancelled`; a repo's `job` is its latest, in any state |
| `POST /api/jobs/<id>/cancel` | the job |
| `GET /api/jobs/<id>/events` | `text/event-stream`: what happened so far first (the log and the latest progress), then `progress` `{phase, done, total, current, cost_usd}`, `log` `{line}`, and at the end `done` `{version}` or `error` `{message}` |
| `GET /api/repos/<key>/wiki[?version=<n>]` | the version's `wiki.json` (below), the latest without `version`; 404 when there's none |
| `GET /api/repos/<key>/status` | `{stale, head, commit, job}`: stale when the repo's branch has moved on since the latest version |
| `POST /api/repos/<key>/ask` `{question, conversation, section}` | `text/event-stream`: `tool` `{name, path}` as Claude reads, `delta` `{text}` as it answers, then `done` `{conversation, cost_usd}` or `error` `{message}` |
| `GET /api/repos/<key>/open?path=&line=` | 204, the file opened in the editor; 404 for a path outside the repo |
| `GET` / `PUT /api/settings` | `{model, concurrency, budget_usd, ask_model, ask_budget_usd, exclude}` |

`wiki.json` is version 1 of the generator's contract: `{version, repo: {name, root, commit, branch, web_url,
code_url}, generated: {at, by, model, cost_usd}, overview: {summary_md, diagram}, sections: [{id, title,
summary_md, diagram, subsections: [{id, title, body_md, diagram, files}]}]}`, a diagram being `{mermaid,
caption}`. Its markdown is CommonMark with GitHub's tables; a link into the code is `[label](code:PATH#L10-L20)`,
one to another part of the page `[label](#id)`. `code_url` is a template (`{commit}`, `{path}`) the lines are
appended to, as `#L10-L20`; null without a forge.

## How lattice serves it

- `npm --prefix web run build` writes the app to `web/build/`: `index.html`, `_app/` (its scripts and styles,
  their names hashed), `fonts/`, `theme.js` and `favicon.svg`; and `export/`, below.
- A GET for a file in it serves the file (`/_app/immutable/` can be cached for good); any other GET outside
  `/api/` serves `index.html`, and the app routes from there. Its paths are absolute, so it's served at the
  root.
- Mermaid isn't bundled: the app loads `/assets/mermaid.min.js`, mermaid 11.17.2, which lattice downloads once,
  checks against its SHA-256 and serves from its cache. It loads it only when the first diagram comes near.
- `index.html` carries a Content-Security-Policy: scripts from the app itself (and the hash of SvelteKit's
  starting script), connections to lattice alone, no frames, no objects. The app fetches nothing from anywhere
  else.

## The export

`lattice export <repo> <dir>` writes a wiki out as a static site that works from `file://` and on any host. The
build makes its parts in `web/build/export/`: `wiki.js` (one classic script, since a page opened from a file
can't load modules), `wiki.css`, `fonts/`, `theme.js`, `favicon.svg` and `index.html`. lattice copies them into
`<dir>`, puts `mermaid.min.js` beside them, and fills the page's `<script type="application/json"
id="wiki-data"></script>` with the wiki.json, every `<` written `<`. There, links into the code go to the
forge (or are plain code without one), and the chat says asking needs `lattice serve`.

## Working on it

```sh
npm --prefix web ci
node web/mock/server.mjs          # a stand-in for lattice's API, on 127.0.0.1:7348
npm --prefix web run dev          # the app on http://localhost:5173, its /api/ proxied to the mock
LATTICE_URL=http://127.0.0.1:7347 npm --prefix web run dev   # or to a real `lattice serve`
```

The mock (`web/mock/server.mjs`) answers the whole API from fixtures and from memory: a hand-written wiki of
crystal with two versions and its code moved on since, a synthetic one of 16 sections and 90 subsections
(`web/mock/synth.mjs`), a build running, one that failed, and a repo never built; jobs started from the app
run, stream and make a version; the chat streams an answer, reading files as it goes ("error" in a question
makes it fail, "slow" slows it); settings save and are checked. It checks Host and Origin as lattice does.
`--pace 5` runs its jobs five times slower; `--app build` serves the built app as lattice does, for
screenshots and measuring.

```sh
npm --prefix web run check        # svelte-check: types, and Svelte's accessibility checks, warnings failing it
npm --prefix web test             # vitest: the markdown renderer, links into the code, fences, mermaid's repairs, the event streams
npm --prefix web run build        # the app, then the export
node web/scripts/perf.mjs http://127.0.0.1:7348/atlas [--cpu 4]   # with the mock serving the build
```

On an Apple M4 Pro Mac, the synthetic wiki (107 diagrams, 3,041 links into the code, 9,856 elements) has its text on
screen 156 ms after it's asked for, and scrolls top to bottom at 1,500 px a second with a median frame of
16.7 ms and none over 50 ms, its diagrams drawn as they come near; with the CPU four times slower, its text is
there in under half a second.

## Its parts

- Dependencies, all at build time: Svelte and SvelteKit with its static adapter (the app, a single-page app),
  Vite (the build), TypeScript and svelte-check (types and Svelte's accessibility checks), vitest (the tests),
  highlight.js (code blocks coloured, loaded the first time a page has one, with the languages a wiki quotes),
  and Node's types for the build's config. Nothing else runs in the browser but mermaid, from lattice.
- The markdown renderer (`src/lib/markdown.ts`) is its own, small and strict: CommonMark's blocks and inlines a
  wiki uses, every other character escaped, raw HTML never passed through, links only to `#`, `code:`, `http`,
  `https` and `mailto`, and images shown as links, never loaded.
- Fences are read by CommonMark's rules (`src/lib/fences.ts`): a fence opens with three or more backticks or
  tildes indented three spaces at most, and closes only on a run of the same character at least as long with
  nothing after it, so a ```` ```mermaid ```` fence quoted inside a longer fence is text, not a diagram. The
  generator's diagram check reads them the same way.
- Diagrams are drawn as they come within about a screen of the window, the nearest first, one at a time
  (`src/lib/mermaid.ts`), after small repairs to their source (`src/lib/mermaid-sanitize.ts`); one mermaid can't
  draw shows its source. Their colours come from the theme's tokens over what mermaid draws, so a change of
  theme needs no redraw.
- Fonts: Google Sans Flex stands in for Google Sans and Google Sans Text, which can't be bundled, beside Google
  Sans Code; both are under the SIL Open Font License.
