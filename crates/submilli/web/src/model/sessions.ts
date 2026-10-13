// The session bar, the sessions dialog, the footer, and what stepping back turns off.
import type { SessionSummary } from "../api/types";
import { agentLabel, capitalize, plural, settingsLabel, short, when } from "./format";
import { sessionRef } from "./refs";
import type { Frame } from "./steps";

/** Why test, rerun, draft and chat are off while the reader is stepped back. */
export const STEPPED_BACK = "Back to latest to use this";

/** Why an action other than Cancel can't be used now, or `null` when it can. */
export function actionBlock(f: Frame): string | null {
  return f.stepping ? STEPPED_BACK : null;
}

export function footer(f: Frame): string {
  if (f.stepping) return "Stepping through · read-only";
  if (f.ui.mode === "setup") return "New session · not started yet";
  if (f.ui.mode === "fresh" || (!f.ended.length && f.info?.open)) return "Live session · waiting for a run";
  return "At the latest event";
}

/** Tests and reruns each run in a throwaway session; those show under the run, not as sessions. */
function throwaway(s: SessionSummary): boolean {
  return s.sources.includes("test") || s.sources.includes("rerun");
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
  vars: string[];
  runs: string;
  denied: boolean;
  /** How many tests and reruns of its runs, as far as they're loaded. */
  follow: string;
}

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
      vars: varsOf(fresh.variables).map((v) => v.full),
      runs: "0 runs",
      denied: false,
      follow: "",
    });
  }
  const listed = f.data.sessions.filter((s) => !throwaway(s)).sort((a, b) => b.started_at_micros - a.started_at_micros);
  for (const s of listed) {
    const current = f.ui.mode === "open" && s.session === f.data.sessionId;
    // A session's task is its first message; only the shown session's events are loaded.
    let task = "No chat loaded";
    let muted = true;
    if (current) {
      const asks = f.steps.flatMap((x) => (x.kind === "chat" && x.role === "user" ? [x.text] : []));
      task = asks.length ? asks[0] + (asks.length > 1 ? " (+" + (asks.length - 1) + " more)" : "") : "No chat";
      muted = !asks.length;
    }
    const follow = s.runs.reduce((sum, n) => sum + (f.data.runs[n]?.reruns.length || 0), 0);
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
      vars: varsOf(s.variables).map((v) => v.full),
      runs: plural(s.runs.length, "run", "runs"),
      denied: s.denied,
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
  source: string;
  agent: string;
  started: string;
  state: string;
  live: boolean;
}

/** The bar above the panes for the session shown, or `null` while a new one is set up. */
export function sessionBar(f: Frame): SessionBar | null {
  const withSettings = (agent: string, settings: string) => agentLabel(agent) + (settings ? " · " + settings : "");
  const fixed = (all: FixedVar[]) => {
    const vars = all.length > 3 ? all.slice(0, 2) : all;
    return { vars, allVars: all, moreVars: all.length > vars.length ? "+" + (all.length - vars.length) + " more" : "" };
  };
  const fresh = f.ui.mode === "fresh" ? f.ui.fresh : null;
  if (fresh) {
    return {
      short: "new session",
      copyText: "",
      ...fixed(varsOf(fresh.variables)),
      source: "Browser",
      agent: withSettings(fresh.agent, settingsLabel(fresh.settings)),
      started: "Started " + when(fresh.started),
      state: "Live",
      live: true,
    };
  }
  const s = f.info;
  if (f.ui.mode !== "open" || !s) return null;
  // A chat's session takes more messages until it ends.
  const open = !!(s.model && s.open);
  return {
    short: short(s.session),
    copyText: sessionRef(f.names, s.session),
    ...fixed(varsOf(s.variables)),
    source: sourceLabel(s),
    agent: s.model ? withSettings(s.model, settingsLabel(s.settings)) : "",
    started: "Started " + when(s.started_at),
    state: open ? "Open" : s.model ? "Ended" : "Finished",
    live: open && !f.stepping,
  };
}
