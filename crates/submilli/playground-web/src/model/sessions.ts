// The session bar, the sessions dialog, the footer, and what stepping back turns off.
import type { ChatSettings, SessionSummary } from "../api/types";
import { agentLabel, capitalize, differing, plural, providerSettings, settingsLabel, short, when, type ValuesNote } from "./format";
import { sessionRef } from "./refs";
import type { Frame } from "./steps";

/** Why test, rerun, draft and chat are off while the reader is stepped back. */
export const STEPPED_BACK = "Back to latest to use this";

/** Why an action other than Cancel can't be used now, or `null` when it can. */
export function actionBlock(f: Frame): string | null {
  return f.stepping ? STEPPED_BACK : null;
}

/** The footer's left side; `waiting` while the agent is answering a message the page sent. */
export function footer(f: Frame, waiting = false): string {
  if (f.stepping) return "Stepping through · read-only";
  if (f.ui.mode === "setup") return "New session · not started yet";
  if (f.running.length) return "Live session · run " + f.running[f.running.length - 1] + " is running";
  if (waiting) return "Live session · waiting for the agent";
  if (f.ui.mode === "fresh") return f.ui.fresh?.messages.length ? "Live session · waiting for the agent" : "New session · send its first message";
  if (f.data.sessionId === null) return "No session yet";
  // As the design has it: an open session at its latest event reads the same as a finished one.
  return "At the latest event";
}

/** Tests and reruns each run in a throwaway session; those show under the run, not as sessions. */
const THROWAWAY = new Set(["test", "rerun"]);

/**
 * When the session shown is open and keeps values that differ from the ones bound now:
 * which differ. A session's values are fixed when it starts, so a new binding never
 * reaches its messages; without this, a chat looks the same while it isn't. Only names
 * bound now count: a value the session has with nothing bound under its name is not
 * listed, while a value bound now that the session lacks is.
 */
export function keptValues(f: Frame): ValuesNote | null {
  // A new session not started yet shows over the last one, which isn't the one in view.
  if (f.ui.mode !== "open") return null;
  const s = f.data.sessions.find((x) => x.session === f.data.sessionId);
  const bound = f.data.status?.binding.variables;
  if (!s || !bound || s.open !== true || isThrowaway(s)) return null;
  const changes = differing(s.variables, bound).filter((change) => Object.hasOwn(bound, change.name));
  return changes.length
    ? { lead: "", changes, tail: "", summary: "New sessions will use different settings." }
    : null;
}

/**
 * Where Back goes after opening a test's or rerun's own session `opening` from `shown`:
 * the session shown, or, when that is itself such a session opened the same way
 * (`current`), the one that opened it. Never `opening` itself, which Back could not leave.
 */
export function wayBack(current: { to: string; from: string | null } | null, shown: string | null, opening: string): string | null {
  const back = current && shown === current.to ? current.from : shown;
  return back === opening ? null : back;
}

/** A session that only holds a test or a rerun, as the playground counts them; the page shows those under their run. */
export function isThrowaway(s: SessionSummary): boolean {
  return s.sources.length > 0 && s.sources.every((source) => THROWAWAY.has(source));
}

/** Where a session came from, as the page names it: the page's own chat is the Browser. */
function sourceLabel(s: SessionSummary): string {
  const src = s.sources[0] || "";
  return src === "browser-chat" ? "Browser" : capitalize(src);
}

export interface FixedVar {
  name: string;
  value: string;
  full: string;
}

function varsOf(vars: Record<string, string>): FixedVar[] {
  return Object.keys(vars).map((k) => ({ name: k, value: vars[k], full: k + "=" + vars[k] }));
}

export interface SessionRow {
  /** `null` for a new session not started yet. */
  id: string | null;
  current: boolean;
  openable: boolean;
  task: string;
  taskMuted: boolean;
  short: string;
  copyText: string;
  started: string;
  source: string;
  agent: string;
  agentSettings: string;
  /** The variables shown, as `name=value`: all of them, or two when there are more than three. */
  vars: string[];
  /** "+2 more", with every variable in its title, or empty. */
  moreVars: string;
  allVars: string[];
  runs: string;
  /** How its runs ended, past completing: "denied", "1 rejected", "2 failed", "1 cancelled", as the log words them. */
  ends: { text: string; tone: "bad" | "quiet" }[];
  /** How many tests and reruns of its runs, as far as they're loaded. */
  follow: string;
}

/** Many values show as two and "+N more"; up to three show whole. */
function withMore<T>(all: T[]): { vars: T[]; allVars: T[]; moreVars: string } {
  const vars = all.length > 3 ? all.slice(0, 2) : all;
  return { vars, allVars: all, moreVars: all.length > vars.length ? "+" + (all.length - vars.length) + " more" : "" };
}

const rowVars = (vars: Record<string, string>) => withMore(varsOf(vars).map((v) => v.full));

/** The sessions dialog's rows: newest first, a new session not started yet at the top. */
export function sessionRows(f: Frame): SessionRow[] {
  const rows: SessionRow[] = [];
  const fresh = f.ui.mode === "fresh" ? f.ui.fresh : null;
  if (fresh) {
    rows.push({
      id: null,
      current: true,
      openable: false,
      task: fresh.messages[0] || "No task yet",
      taskMuted: !fresh.messages[0],
      short: "not started",
      copyText: "",
      started: when(fresh.started),
      source: "Browser",
      agent: agentLabel(fresh.agent),
      agentSettings: settingsLabel(fresh.settings),
      ...rowVars(fresh.variables),
      runs: "0 runs",
      ends: [],
      follow: "",
    });
  }
  const listed = f.data.sessions.filter((s) => !isThrowaway(s)).sort((a, b) => b.started_at_micros - a.started_at_micros);
  for (const s of listed) {
    const current = f.ui.mode === "open" && s.session === f.data.sessionId;
    // A session's task is its first message.
    const first = s.untrusted?.task;
    const task = first ? first + (s.asks > 1 ? " (+" + (s.asks - 1) + " more)" : "") : s.sources.includes("stand-in") ? "No chat captured" : "No chat";
    const muted = !first;
    const follow = s.follow_ups ?? 0;
    rows.push({
      id: s.session,
      current,
      openable: !current,
      task,
      taskMuted: muted,
      short: short(s.session),
      copyText: sessionRef(f.names, s.session),
      started: when(s.started_at),
      source: sourceLabel(s),
      agent: agentLabel(s.model) || "—",
      agentSettings: settingsLabel(s.settings),
      ...rowVars(s.variables),
      runs: plural(s.runs.length, "run", "runs"),
      ends: [
        ...(s.denied ? [{ text: "denied", tone: "bad" as const }] : []),
        ...(s.rejected ? [{ text: s.rejected + " rejected", tone: "bad" as const }] : []),
        ...(s.failed ? [{ text: s.failed + " failed", tone: "bad" as const }] : []),
        ...(s.cancelled ? [{ text: s.cancelled + " cancelled", tone: "quiet" as const }] : []),
      ],
      follow: follow ? String(follow) : "",
    });
  }
  return rows;
}

export interface SessionBar {
  short: string;
  /** The copy-for-assistant line; empty for a session not started on the server yet. */
  copyText: string;
  /** The variables shown in the bar: all of them, or two when there are more than three. */
  vars: FixedVar[];
  /** "+2 more", opening the full list, or empty. */
  moreVars: string;
  allVars: FixedVar[];
  /** Every provider setting, for the dialog; the bar names the first. */
  settings: { name: string; value: string }[];
  /** "All settings", opening them, when the bar doesn't show them whole; or empty. */
  moreSettings: string;
  source: string;
  agent: string;
  started: string;
  state: string;
  live: boolean;
}

/** Every setting of the session's agent, for the dialog: effort, max output, then each provider setting. */
function agentSettings(set: ChatSettings | undefined | null): { name: string; value: string }[] {
  return [
    ...(set?.effort ? [{ name: "effort", value: set.effort }] : []),
    ...(set?.max_output ? [{ name: "max output", value: String(set.max_output) }] : []),
    ...providerSettings(set).map((p) => ({ name: "provider: " + p.name, value: p.value })),
  ];
}

/** The bar above the panes for the session shown, or `null` while a new one is set up. */
export function sessionBar(f: Frame): SessionBar | null {
  const withSettings = (agent: string, settings: string) => agentLabel(agent) + (settings ? " · " + settings : "");
  // Provider settings the bar can't show whole open the same dialog as the variables.
  const provider = (settings: { name: string; value: string }[]) => settings.filter((p) => p.name.startsWith("provider: "));
  const withAll = (bar: Omit<SessionBar, "settings" | "moreSettings">, settings: { name: string; value: string }[]): SessionBar => ({
    ...bar,
    settings,
    moreSettings: provider(settings).length > 1 || provider(settings).some((p) => p.value.length > 24) ? "All settings" : "",
  });
  const fresh = f.ui.mode === "fresh" ? f.ui.fresh : null;
  if (fresh) {
    return withAll({
      short: "new session",
      copyText: "",
      ...withMore(varsOf(fresh.variables)),
      source: "Browser",
      agent: withSettings(fresh.agent, settingsLabel(fresh.settings)),
      started: "Set up " + when(fresh.started),
      // Nothing exists on the playground until its first message.
      state: "Not started",
      live: false,
    }, agentSettings(fresh.settings));
  }
  const s = f.info;
  if (f.ui.mode !== "open" || !s) return null;
  return withAll({
    short: short(s.session),
    copyText: sessionRef(f.names, s.session),
    ...withMore(varsOf(s.variables)),
    source: sourceLabel(s),
    agent: s.model ? withSettings(s.model, settingsLabel(s.settings)) : "",
    started: "Started " + when(s.started_at),
    // A session started for the coding assistant is open too, though only it runs in it.
    state: s.open ? "Open" : s.model ? "Ended" : "Finished",
    live: !!s.open && !f.stepping,
  }, agentSettings(s.settings));
}
