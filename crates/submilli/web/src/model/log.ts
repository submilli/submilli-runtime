// The log pane: a row per run, its permission checks, and the tests and reruns of it.
import type { Explain, RuleRef, Show, ShowDecision } from "../api/types";
import { bpRuleLine } from "./blueprint";
import { capitalize, ctx, end, ordinal, plainBy, plural, size, tone, when, type Tone } from "./format";
import { decisionRef, forAsst, runRef } from "./refs";
import { countWords, followUps } from "./summary";
import { endedRuns, programNumber, type Frame } from "./steps";
import type { UiState } from "./ui";

/** What a button runs: the page acts on it, and `cmd` is the same from the CLI. */
export type Action =
  | { kind: "draft"; decision: string }
  | { kind: "test"; run: number; mode?: "reads-live" | "live" }
  | { kind: "rerun"; run: number };

/** One way to try a program again. */
export interface AgainOption {
  action: Action;
  goal: string;
  what: string;
  label: string;
  /** It runs something real, so it asks first and looks it. */
  caution: boolean;
  cmd: string;
  copyTitle: string;
  note: string;
}

export interface NearMiss {
  text: string;
  /** What this call had and what the rule wanted, when recorded. */
  values: { name: string; value: string }[] | null;
}

/** A line of the blueprint to open, and what it is. */
export interface RuleSpot {
  version: number;
  line: number;
  label: string;
}

export interface DecisionRow {
  id: string;
  ref: string;
  selected: boolean;
  open: boolean;
  title: string;
  status: string;
  tone: Tone;
  meta: string;
  decidedBy: string;
  near: NearMiss[];
  context: string;
  copyText: string;
  refTitle: string;
  inspect: RuleSpot | null;
  inspectLabel: string;
  /** The program line that asked, when recorded. */
  line: number | null;
  /** What to run next, from the CLI. */
  next: string[];
}

export interface FollowRow {
  n: number;
  id: string;
  num: string;
  selected: boolean;
  open: boolean;
  title: string;
  meta: string;
  status: string;
  tone: Tone;
  /** The answer first: did the change get the program past the refusal? */
  verdict: string;
  further: string;
  showFurther: boolean;
  what: string;
  changes: { tone: "ok" | "bad" | ""; text: string }[];
  isStop: boolean;
  stopLead: string;
  stopKey: string;
  liveNote: string;
  /** Test again so unrecorded reads go to the real service, or every call goes. */
  tryReads: Action;
  tryLive: Action;
  /** A later real-reads test of the same program, when there is one. */
  tried: number | null;
  triedLabel: string;
  wentLive: string;
  error: string;
  result: string;
  copyText: string;
  refTitle: string;
  compareCmd: string;
  againHead: string;
  again: AgainOption[];
}

export interface RunRow {
  n: number;
  id: string;
  word: string;
  selected: boolean;
  open: boolean;
  title: string;
  status: string;
  tone: Tone;
  meta: string;
  /** A closed run's error, first line. */
  preview: string;
  error: string;
  result: string;
  copyText: string;
  refTitle: string;
  again: AgainOption[];
  againNote: string;
  decisions: DecisionRow[];
  checksOpen: boolean;
  checksLabel: string;
  checksSum: string;
  closedSum: string;
  follow: FollowRow[];
  followOpen: boolean;
  followLabel: string;
  followSum: string;
}

export interface LogPane {
  count: string;
  rows: RunRow[];
  /** What the pane says when it has no rows, or `null`. */
  empty: string | null;
  runningNote: string;
  allLabel: string;
  /** The rows Expand all opens and closes. */
  ids: string[];
  allOpen: boolean;
}

function ruleName(rule: RuleRef): string {
  return rule.name ? "rule " + rule.name : "rule " + rule.position + " of " + rule.caller;
}

/** What decided a call, in the CLI's terms. */
export function decidedBy(d: ShowDecision, ex: Explain | undefined): string {
  if (ex && ex.decided_by.kind === "default") {
    const why = ex.decided_by.caller_block ? "No rule in " + d.caller + "’s block matched." : d.caller + " has no rules.";
    return "Decided by the default: " + (ex.decided_by.default_action || "") + ". " + why;
  }
  return (d.outcome === "deny" ? "Denied " : "Allowed ") + plainBy(d.decided_by) + ".";
}

/** The rules that nearly matched, and the comparisons that failed. */
export function nearOf(ex: Explain | undefined): NearMiss[] {
  if (!ex) return [];
  return ex.near_misses.map((m, i) => {
    const vals = ((ex.untrusted.near_misses || [])[i] || [])[0] || {};
    const failed = m.failures.map((x) => x.comparison + " (" + x.reason + ")").join("; ");
    const where = "rule " + m.rule.position + " of " + m.rule.caller + ", line " + m.rule.line;
    const name = m.rule.name ? m.rule.name + " (" + where + ")" : where;
    return {
      text: "Nearest rule: " + name + ". Failed: " + failed + ".",
      values: "actual" in vals ? [{ name: "this call", value: JSON.stringify(vals.actual) }, { name: "the rule wanted", value: JSON.stringify(vals.expected) }] : null,
    };
  });
}

/** The blueprint line to open for a decision: the nearest rule for a refusal, else the deciding rule. */
function ruleSpot(f: Frame, r: Show, d: ShowDecision, ex: Explain | undefined): RuleSpot | null {
  if (ex) {
    const v = Number(ex.blueprint_version);
    if (ex.near_misses.length) return { version: v, line: ex.near_misses[0].rule.line, label: "nearest " + ruleName(ex.near_misses[0].rule) };
    const rule = ex.decided_by.rule;
    if (ex.decided_by.kind === "rule" && rule) return { version: v, line: rule.line, label: ruleName(rule) };
  }
  // Without the explain answer, the decision's own words: "by `name`", or "by rule N of `caller`, line L".
  const v = Number(r.blueprint_version);
  const byName = /^by `([^`]+)`$/.exec(d.decided_by);
  const text = f.data.versions[v]?.text;
  if (byName && text) {
    const line = bpRuleLine(text, byName[1]);
    return line ? { version: v, line, label: "rule " + byName[1] } : null;
  }
  const byPlace = /^by rule (\d+) of `([^`]+)`, line (\d+)$/.exec(d.decided_by);
  return byPlace ? { version: v, line: Number(byPlace[3]), label: "rule " + byPlace[1] + " of " + byPlace[2] } : null;
}

function followKind(r: Show): string {
  if (r.rerun_of) return "Rerun, live";
  const m = r.test?.mode;
  return "Test of blueprint v" + r.blueprint_version + (m === "reads-live" ? ", real reads" : m === "live" ? ", live" : "");
}

function againOptions(f: Frame, r: Show, word: string): AgainOption[] {
  // A program that never compiled fails the same way every time, so nothing is offered.
  if (word === "rejected") return [];
  const firstDenied = r.decisions.find((d) => d.outcome === "deny");
  const n = r.run;
  const options: Omit<AgainOption, "copyTitle">[] = [];
  if (firstDenied) {
    const ref = firstDenied.refs[0];
    options.push({
      action: { kind: "draft", decision: ref },
      goal: "Should the agent be allowed this?",
      what: "Change the blueprint first.",
      label: "Draft a rule for check " + ref,
      caution: false,
      cmd: "submilli playground draft-rule " + ref,
      note: "Shows a rule that allows exactly the call refused in check " + ref + ". Nothing changes until you save it. If the refusal is right, there’s nothing to fix.",
    });
  }
  options.push(
    {
      action: { kind: "test", run: n },
      goal: "Changed the blueprint?",
      what: "See whether this program gets through now.",
      label: f.inForce !== null ? "Test against blueprint v" + f.inForce : "Test against the blueprint in force",
      caution: false,
      cmd: "submilli playground test " + n,
      note: "Nothing real happens: replies come from run " + n + "’s recording.",
    },
    {
      action: { kind: "rerun", run: n },
      goal: "Changed the package, or want other variables?",
      what: "Run it for real.",
      label: "Rerun for real…",
      caution: true,
      cmd: "submilli playground rerun " + n,
      note: "Asks first. Uses the variables you’ve set now." + (firstDenied ? " A package change can’t lift a refusal; only the blueprint can." : ""),
    },
  );
  return options.map((o) => ({ ...o, copyTitle: "Copy the CLI command: " + o.cmd }));
}

function decisionRow(f: Frame, r: Show, d: ShowDecision): DecisionRow {
  const ref = d.refs[0];
  const id = "d" + ref;
  const ex = f.data.explains[ref];
  const call = (f.session.calls[r.run] || []).find((c) => c.check?.ref === ref);
  const spot = ruleSpot(f, r, d, ex);
  const copyText = decisionRef(f.names, r, d);
  return {
    id,
    ref,
    selected: f.ui.sel === id,
    open: !!f.ui.open[id],
    title: " · " + d.caller + " → " + d.capability + (d.refs.length > 1 ? " ×" + d.refs.length : ""),
    status: d.outcome === "deny" ? "Denied" : "Allowed",
    tone: d.outcome === "deny" ? "bad" : "ok",
    meta: "blueprint v" + r.blueprint_version + (call && call.line ? " · program line " + call.line + " · " + (call.bytes ? size(call.bytes) + " fetched" : "nothing fetched") : ""),
    decidedBy: decidedBy(d, ex),
    near: nearOf(ex),
    context: ctx(r.untrusted.contexts?.[ref]),
    copyText,
    refTitle: forAsst(copyText),
    inspect: spot,
    inspectLabel: spot ? "Blueprint v" + spot.version + ", line " + spot.line : "",
    line: call ? call.line : null,
    next: ex ? ex.next : [],
  };
}

function followRow(f: Frame, r: Show, fu: Show, again: AgainOption[], all: number[]): FollowRow {
  const n = fu.run;
  const id = "f" + n;
  const w = end(fu.outcome);
  const word = end(r.outcome);
  const cmp = f.data.compares[n];
  const test = fu.test;
  const ut = fu.untrusted.test || {};
  const tried = all.map((x) => f.data.runs[x]).find((x) => x.run > n && x.test?.mode === "reads-live");
  // Lead with the answer: did the change get the program past run r's refusal?
  const fixed = cmp ? cmp.changes.find((c) => c.kind === "newly-allowed") : undefined;
  const rest = cmp ? cmp.changes.filter((c) => c !== fixed) : [];
  const changes: FollowRow["changes"] = rest.map((c) => ({
    tone: c.kind === "newly-allowed" ? "ok" : c.kind === "newly-denied" ? "bad" : "",
    text: "Check " + c.before + " → " + c.after + " " + c.kind.replace("-", " ") + ": " + c.caller + " → " + c.capability + ", now " + plainBy(c.decided_by) + (c.rule ? " (line " + c.rule.line + ")" : "") + ".",
  }));
  if (cmp && !fixed) for (const u of cmp.unmatched) changes.push({ tone: "", text: capitalize(u) + "." });
  const refused = r.decisions.some((d) => d.outcome === "deny");
  const verdict = fixed
    ? "Your change worked: check " + fixed.before + " is now allowed" + (fixed.rule ? " (rule " + fixed.rule.name + ", line " + fixed.rule.line + ")" : "") + "."
    : w === word && cmp && !cmp.changes.length
      ? "Same result as run " + r.run + (refused ? ": refused at the same check, so nothing changed that check." : ".")
      : "";
  const further = !fixed
    ? ""
    : w === "stopped"
      ? "The program then went further than run " + r.run + " did. Its next call, below, has no recorded reply, so the test stopped rather than send it. Nothing was sent."
      : w === "completed"
        ? "The program then went further than run " + r.run + " did, and finished."
        : "The program then went further than run " + r.run + " did.";
  const stopDetail = (ut.stop_detail || "").replace("the recorded run", "run " + r.run);
  const copyText = runRef(f.names, fu);
  return {
    n,
    id,
    num: String(n),
    selected: f.ui.sel === id,
    open: !!f.ui.open[id],
    title: " · " + followKind(fu),
    meta: "blueprint v" + fu.blueprint_version + " · " + when(fu.started_at),
    status: w === "stopped" ? (fixed ? "Got past the refusal · stopped at a new call" : "Stopped · nothing sent") : capitalize(w),
    tone: tone(w),
    verdict,
    further,
    showFurther: !!further && w !== "stopped",
    what: fu.rerun_of
      ? "Run " + r.run + "’s program, run for real with the variables set at the time."
      : "Run " + r.run + "’s program, tested against blueprint v" + fu.blueprint_version + " with run " + r.run + "’s variables. " +
        (test?.mode === "reads-live"
          ? "Reads with no recorded reply went to the real service."
          : test?.mode === "live"
            ? "Calls with no recorded reply went to the real service."
            : "Replies came from run " + r.run + "’s recording, so nothing real happened."),
    changes,
    isStop: w === "stopped",
    stopLead: fixed ? further : "Stopped before this call: " + stopDetail + ", so there was no recorded reply to use. Nothing was sent.",
    stopKey: ut.stop_key || "",
    liveNote: test?.live_note || "",
    tryReads: { kind: "test", run: r.run, mode: "reads-live" },
    tryLive: { kind: "test", run: r.run, mode: "live" },
    tried: tried ? tried.run : null,
    triedLabel: tried ? tried.run + ", " + followKind(tried).toLowerCase() + ", " + end(tried.outcome) : "",
    wentLive: (ut.went_live_keys || []).join("\n"),
    // A stopped test was cancelled at the call; that isn't an error to show.
    error: w === "stopped" ? "" : fu.untrusted.error || "",
    result: fu.untrusted.result || "",
    copyText,
    refTitle: forAsst(copyText),
    compareCmd: "submilli playground compare " + r.run + " " + n,
    againHead: "Try run " + r.run + "’s program again",
    again,
  };
}

function runRow(f: Frame, r: Show, i: number, runs: Show[]): RunRow {
  const n = r.run;
  const id = "r" + n;
  const open = !!f.ui.open[id];
  const word = end(r.outcome);
  const again = againOptions(f, r, word);
  const later = runs[i + 1];
  const againNote =
    word === "rejected" ? "This program didn’t compile, so running it again gives the same error." + (later ? " The agent’s next program is run " + later.run + "." : "") : "";
  const ups = f.stepping ? [] : followUps(f, n);
  const ascending = [...ups].sort((a, b) => a - b);
  const follow = ups.map((x) => followRow(f, r, f.data.runs[x], again, ascending));
  const decisions = r.decisions.map((d) => decisionRow(f, r, d));
  const denials = r.decisions.filter((d) => d.outcome === "deny").length;
  const allowed = r.decisions.length - denials;
  const returned = f.session.returned[n];
  const err = r.untrusted.error || "";
  const copyText = runRef(f.names, r);
  return {
    n,
    id,
    word,
    selected: f.ui.sel === id,
    open,
    title: "Run " + n + " · the agent’s " + ordinal(programNumber(f, n)) + " program",
    status: capitalize(word),
    tone: tone(word),
    meta:
      when(r.started_at) + " · " + r.wall_ms + " ms · blueprint v" + r.blueprint_version +
      (word === "completed" ? " · returned to agent " + size(returned || 0) : " · error sent to agent" + (returned ? ", " + size(returned) : "")),
    preview: !open && err ? err.split("\n")[0] : "",
    error: err,
    result: r.untrusted.result || "",
    copyText,
    refTitle: forAsst(copyText),
    again,
    againNote,
    decisions,
    checksOpen: f.ui.checksOpen[n] ?? denials > 0,
    checksLabel: "Permission checks (" + decisions.length + ")",
    checksSum: [denials ? denials + " denied" : "", allowed ? allowed + " allowed" : ""].filter(Boolean).join(" · "),
    closedSum: [denials ? plural(denials, "check", "checks") + " denied" : "", follow.length ? plural(follow.length, "rerun", "reruns") + " by you" : ""].filter(Boolean).join(" · "),
    follow,
    followOpen: !!f.ui.followOpen[n],
    followLabel: "Reruns by you (" + follow.length + ")",
    followSum: countWords(follow.map((x) => end(f.data.runs[x.n].outcome))),
  };
}

export function logPane(f: Frame): LogPane {
  const runs = endedRuns(f);
  const all = runs.map((r, i) => runRow(f, r, i, runs));
  const filter = f.ui.filter;
  const rows = all.filter((x) => filter === "all" || (filter === "problems" ? x.word !== "completed" || x.decisions.some((d) => d.status === "Denied") : x.follow.length > 0));
  const ids: string[] = [];
  for (const x of rows) {
    ids.push(x.id);
    if (x.open && x.checksOpen) for (const d of x.decisions) ids.push(d.id);
    if (x.open && x.followOpen) for (const fu of x.follow) ids.push(fu.id);
  }
  const allOpen = ids.length > 0 && ids.every((i) => f.ui.open[i]);
  const atCall = f.atCall;
  let empty: string | null = null;
  if (!rows.length && !atCall) {
    empty = f.ui.mode !== "open" || (!f.stepping && !runs.length) ? "No runs yet." : runs.length ? "Nothing matches this filter." : "Step forward to see runs.";
  }
  return {
    count: plural(rows.length, "run", "runs"),
    rows,
    empty,
    runningNote: atCall ? "Run " + atCall.run + ", the agent’s " + ordinal(programNumber(f, atCall.run)) + " program, is running. It appears here when it ends." : "",
    allLabel: allOpen ? "Collapse all" : "Expand all",
    ids,
    allOpen,
  };
}

/** What Expand all (or Collapse all) changes: every row, with its checks, opens or closes. */
export function expandAll(log: LogPane, ui: UiState): Pick<UiState, "open" | "checksOpen"> {
  const open = { ...ui.open };
  const checksOpen = { ...ui.checksOpen };
  for (const i of log.ids) open[i] = !log.allOpen;
  if (!log.allOpen) {
    for (const x of log.rows) {
      checksOpen[x.n] = true;
      for (const d of x.decisions) open[d.id] = true;
    }
  }
  return { open, checksOpen };
}
