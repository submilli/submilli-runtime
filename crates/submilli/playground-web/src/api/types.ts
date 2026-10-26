// What the control API answers, as far as the page reads it. Fields the page doesn't
// use are left out; everything under `untrusted` came from inside a run or a chat and is
// shown only as text.

/** `GET /api/status`. */
export interface Status {
  blueprints?: SessionBlueprint[];
  project: string;
  blueprint: { name: string; path: string };
  url: string;
  /** The models a chat can use, as `<provider>:<model>`, the default first. */
  chat_models: string[];
  packages: PackageEntry[];
  packages_error?: string;
  running: boolean;
  stopping: boolean;
  blueprint_status: { version: number | null; refused: BlueprintRefusal | null };
  binding: { variables: Record<string, string>; secrets: string[] };
  /** Each provider chat works with, the variable its key goes under, and where its key is found now. */
  model_keys?: ModelKey[];
  /** The variables the blueprint in force declares. */
  declared_variables: DeclaredVariable[];
}

export interface SessionBlueprint {
  name: string;
  path: string;
  declared_variables: DeclaredVariable[];
  packages: PackageEntry[];
  packages_error?: string | null;
  blueprint_status: Status["blueprint_status"];
}

export interface DeclaredVariable {
  name: string;
  required: boolean;
  /** Human-facing context supplied by the blueprint author. */
  description?: string;
  /** An illustrative value; the form never binds it automatically. */
  example?: string;
}

export interface PackageEntry {
  name: string;
  version: string;
  origin: string;
  importable: boolean;
  project: boolean;
}

export interface BlueprintRefusal {
  code: string;
  message: string;
  line?: number;
  column?: number;
}

/** `GET /api/sessions`. */
export interface SessionList {
  sessions: SessionSummary[];
  more: number;
}

export interface SessionSummary {
  blueprint?: string;
  session: string;
  started_at: string;
  started_at_micros: number;
  sources: string[];
  variables: Record<string, string>;
  /** The agent, as `<provider>:<model>`, for a session the playground's chat drove. */
  model?: string;
  settings?: ChatSettings;
  run_count: number;
  runs: number[];
  denied: boolean;
  /** How many of its runs were cancelled, how many didn't compile, and how many failed some other way. */
  cancelled: number;
  rejected: number;
  failed: number;
  /** Whether a chat session takes more messages; `null` for one that isn't a chat. */
  open: boolean | null;
  /** How many tests and reruns of its runs there are. */
  follow_ups: number;
  /** How many messages were said to the agent. */
  asks: number;
  /** The first of them, capped: the session's task, as whoever drove it wrote it. */
  untrusted: { task?: string };
}

export interface ChatSettings {
  effort?: string;
  max_output?: number;
  provider?: Record<string, unknown>;
}

/** `GET /api/runs`. */
export interface RunList {
  runs: RunSummary[];
  /** Runs in flight now, newest first; absent when there are none. */
  running?: RunningRow[];
}

export interface RunningRow {
  run: number;
  /** Who started it: `test`, `rerun`, `browser-chat`, … */
  source: string;
  started_at_micros: number;
  session: string | null;
  /** The program it runs, absent for a file tool's call: written by the agent. */
  untrusted?: { program?: string };
}

export interface RunSummary {
  run: number;
  blueprint_version: string;
  source: string;
  started_at: string;
  outcome: Outcome;
  decisions: number;
  denied: string[];
  session: string | null;
  test_of: number | null;
  rerun_of: number | null;
}

/** How a run ended: `completed`, `denied` (with the decision), `failed` (with the error kind), `stopped`, `cancelled`. */
export interface Outcome {
  kind: string;
  decision?: string;
  error?: string;
}

/** `GET /api/runs/{run}?program=true`. */
export interface Show {
  run: number;
  /** Recorded Blueprint identity; older imported records may omit it. */
  blueprint?: string;
  blueprint_version: string;
  source: string;
  started_at: string;
  wall_ms: number;
  outcome: Outcome;
  denied: string[];
  session: string | null;
  variables: Record<string, string>;
  test?: TestInfo;
  rerun_of?: number | null;
  reruns: { run: number; kind: string; blueprint_version: string; outcome: Outcome }[];
  decisions: ShowDecision[];
  calls: { count: number; returned: number; failed: number; unfinished: number; with_bodies: number };
  program_recorded: boolean;
  /** What started it: `program`, `mcp`, `test`, or `mcp:<tool>` for one of the agent's file tools. */
  entry?: string;
  next: string[];
  untrusted: {
    contexts?: Record<string, Record<string, unknown>>;
    error?: string;
    result?: string;
    program?: string;
    test?: { stop_key?: string; stop_detail?: string; served_keys?: string[]; went_live_keys?: string[] };
  };
}

export interface TestInfo {
  source_run: number;
  status: string;
  mode: 'recorded' | 'reads-live' | 'live';
  stopped?: { line?: number; column?: number };
  live_note?: string;
  local_state?: string;
  variables_filled: string[];
  variables_dropped: string[];
}

export interface ShowDecision {
  refs: string[];
  outcome: 'allow' | 'deny';
  caller: string;
  capability: string;
  /** In the CLI's words: "by `rule-name`", "by the default; near misses: …". */
  decided_by: string;
  filtered: boolean;
}

/** `GET /api/decisions/{ref}`. */
export interface Explain {
  run: number;
  blueprint_version: string;
  decision: string;
  outcome: 'allow' | 'deny';
  caller: string;
  capability: string;
  decided_by: { kind: 'rule' | 'default'; default_action?: string; caller_block?: boolean; rule?: RuleRef };
  near_misses: { rule: RuleRef; filter: string; failures: { comparison: string; reason: string }[] }[];
  next: string[];
  untrusted: { context?: Record<string, unknown>; near_misses?: { actual?: unknown; expected?: unknown }[][] };
}

export interface RuleRef {
  caller: string;
  index: number;
  position: number;
  /** `null` for a rule written without a name. */
  name: string | null;
  line: number;
  column: number;
  end_line: number;
}

/** `GET /api/compare/{a}/{b}`. */
export interface Compare {
  before: { run: number; blueprint_version: string };
  after: { run: number; blueprint_version: string };
  changes: { kind: string; before: string; after: string; caller: string; capability: string; decided_by: string; rule?: RuleRef }[];
  unmatched: string[];
}

/** `GET /api/changes?version=N`. */
export interface Changes {
  versions: BlueprintVersion[];
  voided: number[];
}

export interface BlueprintVersion {
  version: number;
  at: string;
  classification: string;
  summary: string;
  current: boolean;
  after_run?: number;
  /** Present when one version was asked for. */
  text?: string;
}

/** `GET /api/packages/source`. */
export interface PackageSource {
  package: string;
  module: string;
  bytes: number;
  truncated: boolean;
  /** When the text was cut, how much of the module it holds. */
  note?: string | null;
  untrusted: { text: string };
}

/** `GET /api/packages/docs`. Authored package Markdown is untrusted display text. */
export interface PackageDocs {
  package: string;
  bytes: number;
  truncated: boolean;
  note?: string | null;
  untrusted: { markdown: string };
}

/** What an action route (test, rerun, cancel, draft-rule, agent) answers, always HTTP 200. */
export interface ActionAnswer<T = Record<string, unknown>> {
  /** 0 when it worked; anything else is a refusal or a failure, explained by `result.error`. */
  exit: number;
  result: T & { error?: { kind: string; message: string }; next?: string[] };
  text: string;
}

/** `POST /api/draft-rule`'s result. */
export interface Draft {
  decision: string;
  blueprint_version: string;
  /** Where the rule would go in the blueprint file in force: first and last line. */
  lines: [number, number];
  file_in_project: string;
  untrusted: { rule: string };
}

/** `POST /api/recheck`'s result: a run's checks decided again under the blueprint in force. Nothing runs. */
export interface Recheck {
  run: number;
  /** The version the checks were decided under, when the change log says. */
  in_force: string | null;
  newly_allowed: number;
  newly_denied: number;
  different_rule: number;
  /** Checks whose recording was too cut to decide again. */
  cant_tell: string[];
  changes: { decision: string; change: "newly-allowed" | "newly-denied" | "different-rule"; caller: string; capability: string; now_by: string }[];
}

/** `POST /api/agent`'s result. */
export interface AgentResult {
  session: string;
  continued: boolean;
  provider: string;
  model: string;
  stopped: string;
  tool_calls: number;
  untrusted: { reply?: string };
}

// ---- the session feed ----

/** One line of a session's event log, as `GET /api/sessions/{id}/events` sends it. */
export interface StoredEvent {
  session_seq: number;
  event_id: string;
  /** Where the server placed the event: the order to read a session in. */
  position: EventPosition;
  /** The playground's run number, for an event of a run. */
  run?: number;
  backfilled?: boolean;
  /** For a decision, its number in its run: the `n` in `<run>.<n>`. */
  decision?: number;
  body: EventBody;
}

export interface EventPosition {
  at_micros: number;
  run?: number;
  call_index?: number;
  rank: number;
}

export type EventBody =
  | { event: SessionEvent }
  | { gap: Gap }
  | { chat: ChatTurn }
  | { 'chat-replaced': { turns: string[] } }
  | { 'chat-tools': ChatTools };

/** Events of a run the server dropped under load. */
export interface Gap {
  run_id: string;
  /** How many were dropped; `null` when even the run's end was lost. */
  dropped: number | null;
  /** How many were recovered from the run's record and follow this entry. */
  recovered: number;
  /** The run's record was itself cut, so some decisions or calls are gone for good. */
  lost: boolean;
}

export interface ChatTurn {
  turn: string;
  role: 'user' | 'assistant';
  text: string;
  agent?: string;
  model?: string;
}

export interface ChatTools {
  turn: string;
  calls: { id: string; name: string; arguments?: Record<string, unknown>; arguments_text?: string }[];
  results: { call_id: string; name: string; content: string; truncated?: boolean }[];
}

export interface Line {
  line: number;
  column: number;
}

export interface PackageLine extends Line {
  package: string;
  module: string;
  function?: string;
}

/** A server event; `kind` says which fields it has. */
export interface SessionEvent {
  kind: string;
  at_micros: number;
  run_id?: string;
  call_index?: number;
  caller?: string;
  capability?: string;
  line?: Line;
  package_line?: PackageLine;
  outcome?: string;
  result_bytes?: number;
  bytes?: number;
  wall_ms?: number;
  tool?: string;
  ok?: boolean;
  label?: string;
  [k: string]: unknown;
}

/** The feed's own events, which end or punctuate the stream. */
export type FeedSignal =
  | { kind: 'run-idle'; session: string }
  | { kind: 'session-ended'; session: string }
  | { kind: 'feed-error'; session: string; message: string };

// ---- what the store holds and the page's wording reads ----

/**
 * Everything loaded for the page: the store fills it, and the wording and step-through
 * read it. Maps hold only what has been loaded so far; a missing entry means "not yet".
 */
export interface PageData {
  status: Status | null;
  /** Newest first, as `GET /api/sessions` lists them. */
  sessions: SessionSummary[];
  /** The session shown, or `null` when there is none yet. */
  sessionId: string | null;
  /** The shown session's events, in `session_seq` order, with no duplicates. */
  events: StoredEvent[];
  /** Whether the feed has ended: the session ended, or the feed failed. */
  feedEnded: boolean;
  /** Runs by number: the session's own, and the tests and reruns of them. */
  runs: Record<number, Show>;
  /** The programs of the shown session's runs still in flight, by number, until each run's record is read. */
  running: Record<number, string>;
  /** Decisions by `<run>.<n>`. */
  explains: Record<string, Explain>;
  /** Compares of a follow-up against the run it followed, by the follow-up's number. */
  compares: Record<number, Compare>;
  /** Blueprint versions by number; `text` once that version was asked for. */
  versions: Record<number, BlueprintVersion>;
  /** Package sources by `sourceKey(package, module)`. */
  sources: Record<string, PackageSource>;
  /** Authored package documentation, by package name. */
  packageDocs: Record<string, PackageDocs>;
}

export function sourceKey(pkg: string, module: string): string {
  return pkg + '\u0000' + module;
}

/** Where a model key is found: the shell, the project's `.env`, or the local secret store. */
export type KeySource = "environment" | "env_file" | "secret_store";

export interface ModelKey {
  provider: string;
  variable: string;
  /** The model a session with this provider gets unless it names one. */
  default_model?: string;
  from: KeySource | null;
}

/** `POST /api/model-key`: the variable the key was saved under, and where chat finds the provider's key now. */
export interface SavedKey {
  variable: string;
  from: KeySource | null;
}

/** Documentation and its failures belong to a blueprint/package pair. */
export function packageDocsKey(blueprint: string | undefined, pkg: string): string {
  return JSON.stringify([blueprint ?? null, pkg]);
}
