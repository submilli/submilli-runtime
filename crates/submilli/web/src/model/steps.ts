// The step list a session is replayed by, and the cursor over it.
//
// Stepping goes through each message, each call a program makes at the line that made it,
// and each run's end. A call made inside a package is two steps: the program's line that
// calls into the package (once, while the program stays on that line), then the package's
// own line.
import type { PackageLine, PageData, SessionEvent, SessionSummary, StoredEvent } from "../api/types";
import { askedFor, ctx, sourceRunOf } from "./format";
import { namesOf, type Names } from "./refs";
import type { UiState } from "./ui";

export type Step =
  | { kind: "chat"; key: string; role: "user" | "assistant"; text: string }
  /** One of the agent's tools that made no run; `args`, once its turn is logged, are what it asked. */
  | { kind: "tool"; key: string; tool: string; bytes: number; args?: Record<string, unknown> }
  /** A program line making a call; `into` when the call is made inside a package. */
  | { kind: "call"; key: string; run: number; index: number; into: boolean }
  /** The package's own line for a call made inside a package. */
  | { kind: "package"; key: string; run: number; index: number }
  | { kind: "run"; key: string; run: number };

export type CallStep = Extract<Step, { kind: "call" | "package" }>;

/** One call a run made, from its `call-started` and `call-finished` events. */
export interface Call {
  run: number;
  index: number;
  caller: string;
  capability: string;
  /** The program line that made it, when recorded. */
  line: number | null;
  /** Where inside a package it was made, when recorded. */
  pkg: PackageLine | null;
  outcome: string | null;
  bytes: number;
  /** The blueprint's check of it, when one was recorded. */
  check: Check | null;
}

/** A decision as its event recorded it: `ref` is `<run>.<n>`. */
export interface Check {
  ref: string;
  caller: string;
  capability: string;
  allowed: boolean;
}

export interface Session {
  steps: Step[];
  /** Each run's calls, by call index. */
  calls: Record<number, Call[]>;
  /** What each run returned to the agent, in bytes. */
  returned: Record<number, number>;
  /** How each run ended, as its `run-finished` event says, for runs whose answer isn't loaded. */
  finished: Record<number, { error?: string }>;
  /** Who started each run, from its `run-started` event, before its record is read. */
  labels: Record<number, string>;
}


// Rust's derived order on the server's position: time, then run (none first), call, rank.
function comparePositions(a: StoredEvent, b: StoredEvent): number {
  const pa = a.position;
  const pb = b.position;
  if (pa && pb) {
    const opt = (x: number | undefined, y: number | undefined) => (x === y ? 0 : x === undefined ? -1 : y === undefined ? 1 : x - y);
    const d = pa.at_micros - pb.at_micros || opt(pa.run, pb.run) || opt(pa.call_index, pb.call_index) || pa.rank - pb.rank;
    if (d) return d;
  }
  return a.session_seq - b.session_seq;
}

function eventOf(e: StoredEvent): SessionEvent | null {
  return "event" in e.body ? e.body.event : null;
}

/** Reads a session's events into its steps, each run's calls, and what each run returned. */
export function readSession(events: StoredEvent[]): Session {
  // The conversation as it stands: a replaced turn's live copy gives way to the complete
  // copy logged after it. Replacement follows the order events were appended.
  const bySeq = [...events].sort((a, b) => a.session_seq - b.session_seq);
  const current = new Map<string, StoredEvent>();
  for (const e of bySeq) {
    if ("chat" in e.body) current.set(e.body.chat.turn, e);
    else if ("chat-replaced" in e.body) for (const turn of e.body["chat-replaced"].turns) current.delete(turn);
  }
  const kept = new Set(current.values());

  const calls: Record<number, Call[]> = {};
  const returned: Record<number, number> = {};
  const finished: Record<number, { error?: string }> = {};
  const labels: Record<number, string> = {};
  const callOf = (run: number, index: number): Call => {
    const list = (calls[run] ||= []);
    let c = list.find((x) => x.index === index);
    if (!c) {
      c = { run, index, caller: "", capability: "", line: null, pkg: null, outcome: null, bytes: 0, check: null };
      list.push(c);
      list.sort((x, y) => x.index - y.index);
    }
    return c;
  };

  const steps: Step[] = [];
  // The program line each run's last call was made at, so a package's calls from one line
  // step into the package once.
  const lastLine: Record<number, number> = {};
  // A file tool's run sends no size of its own: its tool call, logged after the run ends,
  // says what went back to the agent.
  let lastEnded: number | null = null;
  // The tool steps still waiting for their turn's record of what each was asked.
  const unasked: { step: Extract<Step, { kind: "tool" }>; tool: string }[] = [];
  for (const e of [...events].sort(comparePositions)) {
    if ("chat-tools" in e.body) {
      for (const call of e.body["chat-tools"].calls) {
        const at = unasked.findIndex((u) => u.tool === call.name);
        if (at < 0) continue;
        if (call.arguments) unasked[at].step.args = call.arguments;
        unasked.splice(at, 1);
      }
      continue;
    }
    if ("chat" in e.body) {
      if (kept.has(e)) steps.push({ kind: "chat", key: "turn:" + e.body.chat.turn, role: e.body.chat.role, text: e.body.chat.text });
      continue;
    }
    const ev = eventOf(e);
    if (!ev) continue;
    const key = "ev:" + e.event_id;
    const run = e.run;
    if (ev.kind === "tool-call" && typeof ev.tool === "string") {
      if (!ev.tool.endsWith("__execute")) {
        const step: Extract<Step, { kind: "tool" }> = { kind: "tool", key, tool: ev.tool.split("__").pop() || ev.tool, bytes: ev.result_bytes || 0 };
        steps.push(step);
        unasked.push({ step, tool: ev.tool });
        if (lastEnded !== null && returned[lastEnded] === undefined) returned[lastEnded] = ev.result_bytes || 0;
      }
      lastEnded = null;
    }
    if (run === undefined) continue;
    if (ev.kind === "run-started" && typeof ev.label === "string") labels[run] = ev.label;
    if (ev.kind === "call-started" && ev.call_index !== undefined) {
      const c = callOf(run, ev.call_index);
      c.caller = ev.caller || "";
      c.capability = ev.capability || "";
      c.line = ev.line ? ev.line.line : null;
      c.pkg = ev.package_line || null;
      if (c.line === null) continue;
      if (!c.pkg || lastLine[run] !== c.line) steps.push({ kind: "call", key, run, index: c.index, into: !!c.pkg });
      lastLine[run] = c.line;
      if (c.pkg) steps.push({ kind: "package", key: key + ":pkg", run, index: c.index });
    } else if (ev.kind === "call-finished" && ev.call_index !== undefined) {
      const c = callOf(run, ev.call_index);
      c.outcome = ev.outcome || null;
      c.bytes = ev.result_bytes || 0;
    } else if (ev.kind === "decision" && e.decision !== undefined) {
      const record = ev.record as { call_index?: number; caller?: string; capability?: string; action?: string } | undefined;
      if (record && record.call_index !== undefined) {
        callOf(run, record.call_index).check = {
          ref: run + "." + e.decision,
          caller: record.caller || "",
          capability: record.capability || "",
          allowed: record.action !== "deny",
        };
      }
    } else if (ev.kind === "run-finished") {
      finished[run] = typeof ev.error === "string" ? { error: ev.error } : {};
      steps.push({ kind: "run", key, run });
      lastEnded = run;
    } else if (ev.kind === "returned") {
      returned[run] = ev.bytes || 0;
    }
  }
  return { steps, calls, returned, finished, labels };
}

/** The runs that have started and not finished, oldest first: what Cancel can stop. */
export function runsInFlight(events: StoredEvent[]): number[] {
  const started: number[] = [];
  const finished = new Set<number>();
  for (const e of events) {
    const ev = eventOf(e);
    if (!ev || e.run === undefined) continue;
    if (ev.kind === "run-started" && !started.includes(e.run)) started.push(e.run);
    else if (ev.kind === "run-finished") finished.add(e.run);
  }
  return started.filter((n) => !finished.has(n));
}

// ---- the cursor

/** The cursor before the first step. Step keys start with `turn:` or `ev:`, so it can't clash. */
export const AT_START = "start";

/**
 * How many steps the reader has reached. A key that is gone (it can't be found any more)
 * reads as the latest, so the page never shows a position that doesn't exist.
 */
export function reached(steps: Step[], cursor: string | null): number {
  if (cursor === null) return steps.length;
  if (cursor === AT_START) return 0;
  const i = steps.findIndex((s) => s.key === cursor);
  return i < 0 ? steps.length : i + 1;
}

// The cursor for having reached `count` steps: reaching the last is being at the latest.
function cursorFor(steps: Step[], count: number): string | null {
  if (count <= 0) return steps.length ? AT_START : null;
  if (count >= steps.length) return null;
  return steps[count - 1].key;
}

export function prevKey(steps: Step[], cursor: string | null): string | null {
  return cursorFor(steps, reached(steps, cursor) - 1);
}

export function nextKey(steps: Step[], cursor: string | null): string | null {
  return cursorFor(steps, reached(steps, cursor) + 1);
}

/** Where Play starts: over from the start when at the latest, else where the reader is. */
export function playFrom(steps: Step[], cursor: string | null): string | null {
  return reached(steps, cursor) >= steps.length ? cursorFor(steps, 0) : cursor;
}

export interface StepBar {
  position: string;
  atStart: boolean;
  atEnd: boolean;
  /** Stepped back from the latest: the page is read-only and offers Back to latest. */
  stepping: boolean;
  /** What a screen reader hears as the reader steps. */
  announce: string;
}

export function stepBar(steps: Step[], cursor: string | null): StepBar {
  const n = steps.length;
  const c = reached(steps, cursor);
  return {
    position: c + " / " + n,
    atStart: n === 0 || c === 0,
    atEnd: n === 0 || c >= n,
    stepping: c < n,
    announce: c < n && c > 0 ? "Event " + c + " of " + n : "",
  };
}

// ---- what every pane reads

/** The record as the reader sees it at the cursor: what every pane's wording reads. */
export interface Frame {
  data: PageData;
  ui: UiState;
  names: Names;
  session: Session;
  steps: Step[];
  /** The steps reached so far. */
  vis: Step[];
  bar: StepBar;
  /** Stepped back while viewing a session. */
  stepping: boolean;
  /** The shown session's summary from the sessions list, when listed. */
  info: SessionSummary | null;
  /** The session's runs in the order they started; a run's program number is its place here. */
  allRuns: number[];
  /** The runs that have ended by the cursor, in order. */
  ended: number[];
  /** The call step at the cursor, while a program is running there. */
  atCall: CallStep | null;
  /** At the latest event: the runs started and not ended yet, oldest first. */
  running: number[];
  /** The blueprint version in force. */
  inForce: number | null;
}

export function frame(data: PageData, ui: UiState): Frame {
  const viewing = ui.mode === "open";
  const session = viewing ? readSession(data.events) : { steps: [], calls: {}, returned: {}, finished: {}, labels: {} };
  const steps = session.steps;
  const vis = steps.slice(0, reached(steps, ui.cursor));
  const bar = stepBar(steps, ui.cursor);
  const allRuns: number[] = [];
  for (const s of steps) if ((s.kind === "call" || s.kind === "package" || s.kind === "run") && !allRuns.includes(s.run)) allRuns.push(s.run);
  // A run that hasn't made a call yet has no step, but it is running all the same.
  const running = viewing && !bar.stepping ? runsInFlight(data.events).filter((n) => !session.finished[n] && !data.runs[n]) : [];
  for (const n of running) if (!allRuns.includes(n)) allRuns.push(n);
  const last = vis[vis.length - 1];
  return {
    data,
    ui,
    names: namesOf(data.status),
    session,
    steps,
    vis,
    bar,
    stepping: viewing && bar.stepping,
    info: (viewing && data.sessions.find((s) => s.session === data.sessionId)) || null,
    allRuns,
    ended: vis.flatMap((s) => (s.kind === "run" ? [s.run] : [])),
    atCall: last && (last.kind === "call" || last.kind === "package") ? last : null,
    running,
    inForce: data.status ? data.status.blueprint_status.version : null,
  };
}

/** Runs the coding assistant or the page started in the session, not the agent. */
const NOT_THE_AGENT = new Set(["assistant", "example", "test", "rerun"]);

/** "your rerun of run 1" or "your test of run 2", for a run that was one; otherwise `null`. */
export function followUpName(f: Frame, run: number): string | null {
  const show = f.data.runs[run];
  const source = show?.source ?? f.session.labels[run];
  if (source !== "test" && source !== "rerun") return null;
  const of = show ? sourceRunOf(show) : null;
  return "your " + source + (of !== null && of !== undefined ? " of run " + of : "");
}

/** Whether the agent ran this program, rather than the developer's coding assistant. */
export function byAgent(f: Frame, run: number): boolean {
  // A run still going has no record yet; its start says who began it.
  const source = f.data.runs[run]?.source ?? f.session.labels[run];
  return source === undefined || !NOT_THE_AGENT.has(source);
}

/** The agent's file tool a run was, such as `list_files`, when it wasn't a program. */
export function fileTool(f: Frame, run: number): string | null {
  const entry = f.data.runs[run]?.entry;
  // As people read it: `submilli__files__list` is files.list.
  return entry && entry.startsWith("mcp:") ? entry.slice(4).replace(/^submilli__/, "").split("__").join(".") : null;
}

/**
 * What a file tool's run asked for, from what its check saw: `text` at a glance, as
 * "/billing", and `all` its fields, for hover text. Empty for a program, whose checks
 * each say what they asked. It tells apart calls to one tool decided differently.
 */
export function callTarget(f: Frame, run: number): { text: string; all: string } {
  if (!fileTool(f, run)) return { text: "", all: "" };
  const contexts = f.data.runs[run]?.untrusted.contexts;
  const first = contexts ? Object.values(contexts)[0] : undefined;
  return { text: askedFor(first), all: ctx(first) };
}

/** A run's place among the agent's programs, from 1; 0 for a run that wasn't one. */
export function programNumber(f: Frame, run: number): number {
  if (!byAgent(f, run) || fileTool(f, run)) return 0;
  return f.allRuns.filter((n) => byAgent(f, n) && !fileTool(f, n)).indexOf(run) + 1;
}

/** The runs that have ended by the cursor whose answers are loaded. */
export function endedRuns(f: Frame) {
  return f.ended.flatMap((n) => (f.data.runs[n] ? [f.data.runs[n]] : []));
}
