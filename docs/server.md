# The server and its API

`lattice serve` (or `lattice open`, in the background) serves the web app and the API it uses, on 127.0.0.1
only. Every address that isn't the API's is the web app's: `/`, `/settings`, `/<key>`, `/<key>/v/<n>` and
`/jobs/<id>` are its pages, and its files are built into the binary. `/assets/mermaid.min.js` is mermaid,
downloaded once at a pinned version and checked against its SHA-256 (see [configuration.md](configuration.md)).

## Keeping other sites out

- It listens on 127.0.0.1 alone, and answers only a `Host` of `127.0.0.1` or `localhost`, which keeps out a site
  that points its name at this machine.
- A request that changes something (a `POST`, `PUT` or `DELETE`, or opening a file) must come from the server's
  own page, as its `Origin` and `Sec-Fetch-Site` say.
- A request's head may be 16 KiB at most, and its body 64 KiB. A path that steps out of where it is (`..`) is
  refused. A repository is named by its key, a job by its number, and a file to open must be in the repository,
  not out of it through a link.

## The API

Errors are JSON, `{"message": "..."}`, with the status that fits: 400 for a request that doesn't make sense,
403 from another site, 404 for what isn't there, 409 for what can't be done now. Times are RFC 3339 in UTC.

| Call | What it does |
| --- | --- |
| `GET /api/repos` | Every repository: `[{"key", "name", "source": {"kind": "local", "path"} or {"kind": "git", "url"}, "root", "versions": [VERSION], "job": JOB or null}]`: `root` is where its code is here, the path it was given or lattice's clone, which the page builds the editors' links from; `job` is its latest. |
| `POST /api/repos` `{"source"}` | Adds a repository by a path (from your home directory unless it's absolute), a git URL or `owner/repo`: `{"key"}`. The same repository added again is the same key. |
| `DELETE /api/repos/<key>` | Forgets it, stopping its job, and takes away its clone and its versions. A local repository is never touched. 204. |
| `POST /api/repos/<key>/jobs` `{"kind", "model", "concurrency"}` | Asks for a job: `kind` is `build`, `sync`, `resume` or `regenerate`; `model` and `concurrency` are null for the settings'. Gives JOB. 409 while the repository has a job not over, for a sync with no version, and for a resume with nothing to resume. |
| `GET /api/jobs/<id>` | JOB. |
| `POST /api/jobs/<id>/cancel` | Cancels it: one waiting never runs, one running stops. Gives JOB. |
| `GET /api/jobs/<id>/events` | `text/event-stream`: everything the job has said so far, then each thing as it says it, until it ends: `progress` PROGRESS, `log` `{"line"}`, then `done` `{"version"}` or `error` `{"message"}` (`"cancelled"` for one cancelled). A job that's over sends what was kept of it. |
| `GET /api/repos/<key>/wiki?version=<n>` | A version's `wiki.json`, the latest without `version`. 404 before the first. |
| `GET /api/repos/<key>/status` | `{"stale", "head", "commit", "job"}`: whether the branch has moved on (`head`) since the latest version (`commit`). A git repository is fetched now and then for it. |
| `POST /api/repos/<key>/ask` `{"question", "conversation", "section"}` | The chat, as `text/event-stream`: `delta` `{"text"}` as the answer is written, `tool` `{"name", "path", "pattern"}` as Claude reads the code, then `done` `{"conversation", "cost_usd"}` or `error` `{"message"}`. `conversation` follows up in an earlier answer's; `section` is the id of the section being read. |
| `GET /api/repos/<key>/open?path=&line=` | Opens the file in your editor at the line: `$VISUAL`, or else `$EDITOR`, when it has a window of its own (VS Code and its kin, Zed, Sublime Text, gvim, MacVim). The page asks it when `open_code_in` is `editor`; the other editors it opens by their links. 204. |
| `GET /api/settings`, `PUT /api/settings` | The settings, as [configuration.md](configuration.md) lists them; a `PUT` takes them all, checks them and saves them, and gives them back. |
| `GET /api/server` | `{"lattice": "<version>"}`, for `lattice open` to know its own. |

```
JOB      {"id", "repo", "kind", "model", "concurrency", "state": "queued|running|done|failed|cancelled",
          "progress": PROGRESS or null, "error" (when failed), "version" (when done),
          "created", "started", "finished"}
PROGRESS {"phase": "plan|write|link|overview", "done", "total", "current", "cost_usd"}
VERSION  {"n", "commit", "branch", "model", "at", "cost_usd"}
```

## Jobs

The server runs one job a repository at a time, three repositories at once. A job holds its repository's lock
while it runs, so a `lattice build` on the command line and the server never build the same repository together.
It prepares the code, runs the generator in a directory of its own, `work/`, keeping its log there, and makes that
directory the next version once the generator has written `wiki.json`. A job that fails or is cancelled leaves
`work/` for a resume. A server that stops cancels the jobs it runs. When a server starts, the jobs left running by
one that died are marked failed, so they can be resumed.
