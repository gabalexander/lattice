// What lattice's server says, as the app reads it: its HTTP API and wiki.json, version 1 of the contract the
// generator writes (docs/web.md says where each comes from).

/** A model as the app offers it: the `claude --model` alias. */
export type Model = 'sonnet' | 'opus';

export type Source = { kind: 'local'; path: string } | { kind: 'git'; url: string };

/** One build of a repo's wiki, kept: `n` counts from 1. */
export interface Version {
  n: number;
  commit: string;
  branch: string | null;
  model: string;
  at: string;
  cost_usd: number | null;
}

export type JobKind = 'build' | 'sync' | 'resume' | 'regenerate';
export type JobState = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';
export type Phase = 'plan' | 'write' | 'link' | 'overview';

export interface Progress {
  phase: Phase;
  done: number;
  total: number;
  /** What it's working on: the title of the subsection being written. */
  current: string | null;
  cost_usd: number | null;
}

export interface Job {
  id: number;
  repo: string;
  kind: JobKind;
  state: JobState;
  /** The model it writes with, and how many subsections at once, when it was given them. */
  model?: string | null;
  concurrency?: number | null;
  created?: string | null;
  progress: Progress | null;
  started: string | null;
  finished: string | null;
  /** Why it failed, when it did. */
  error?: string | null;
  /** The version it made, when it's done. */
  version?: number | null;
}

export interface Repo {
  key: string;
  name: string;
  source: Source;
  /** Where its code is on lattice's machine: the path it was given, or lattice's clone. */
  root: string;
  versions: Version[];
  job: Job | null;
}

export interface RepoStatus {
  /** The repo's branch has moved on since the latest version. */
  stale: boolean;
  head?: string | null;
  commit?: string | null;
  job?: Job | null;
}

/** Where a click on a name in the code opens it: an editor by its link, lattice's `$VISUAL` or `$EDITOR`, or the
 * forge (codelinks.ts). */
export type OpenIn =
  | 'vscode'
  | 'cursor'
  | 'zed'
  | 'intellij'
  | 'pycharm'
  | 'goland'
  | 'webstorm'
  | 'clion'
  | 'rider'
  | 'phpstorm'
  | 'rubymine'
  | 'editor'
  | 'forge';

/** The settings; a model may also be a full name the config file gives, like `claude-opus-5-5`. */
export interface Settings {
  model: string;
  concurrency: number;
  budget_usd: number;
  ask_model: string;
  ask_budget_usd: number;
  exclude: string[];
  open_code_in: OpenIn;
}

/** An event on an answer's stream, `POST /api/repos/<key>/ask`. */
export type AskEvent =
  | { event: 'delta'; data: { text: string } }
  | { event: 'tool'; data: { name: string; path?: string; pattern?: string; command?: string } }
  | { event: 'done'; data: { conversation: string; cost_usd?: number } }
  | { event: 'error'; data: { message: string } };

// ---- wiki.json, version 1 ---------------------------------------------------------------------------------

export interface Diagram {
  mermaid: string;
  caption?: string | null;
}

export interface WikiRepo {
  name: string;
  root?: string | null;
  commit: string;
  branch?: string | null;
  /** The repo on its forge, or null without one. */
  web_url?: string | null;
  /** Where a file is on the forge: `{commit}` and `{path}` filled in, `#L10-L20` appended. */
  code_url?: string | null;
}

export interface Subsection {
  id: string;
  title: string;
  body_md: string;
  diagram?: Diagram | null;
  files?: string[];
}

export interface Section {
  id: string;
  title: string;
  summary_md: string;
  diagram?: Diagram | null;
  subsections: Subsection[];
}

export interface Wiki {
  version: number;
  repo: WikiRepo;
  generated: { at: string; by?: string | null; model?: string | null; cost_usd?: number | null; [k: string]: unknown };
  overview: { summary_md: string; diagram?: Diagram | null };
  sections: Section[];
}
