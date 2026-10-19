// The log pane: a row per run, its permission checks, and the tests and reruns of it.
import type { Explain, RuleRef, Show, ShowDecision } from "../api/types";
import { bpRuleLine } from "./blueprint";
import { askedFor, boundVars, capitalize, ctx, differing, duration, end, ordinal, plainBy, plural, size, tone, when, type Tone, type ValuesNote } from "./format";
import { decisionRef, forAsst, runRef } from "./refs";
import { countWords, followUps } from "./summary";
import { byAgent, callTarget, endedRuns, fileTool, followUpName, programNumber, type Frame } from "./steps";
import type { UiState } from "./ui";

/** What a button runs: the page acts on it, and `commandOf` gives the same from the CLI. */
export type Action =
  | { kind: "draft"; decision: string }
  | { kind: "test"; run: number; mode?: "reads-live" | "live" }
  | { kind: "recheck"; run: number }
  | { kind: "rerun"; run: number };

/** One way to try a program again. */
export interface AgainOption {
  action: Action;
  goal: string;
  what: string;
  label: string;
  /** It runs something real, so it asks first and looks it. */
  caution: boolean;
  /** What its copy button copies: a line for your coding assistant, or the same from the CLI. */
  copy: { label: string; text: string; title: string; what: "reference" | "command" };
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
  /** Every field the check saw, for hover text. */
  hint: string;
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
  /** Which values differ: a rerun's from the run's, or the bound ones from a test's; null when none do. */
  values: ValuesNote | null;
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
  /** How much of a cut result or error shows; "" when it shows whole. */
  cut: string;
  /** Whether it shows whole after Show all, so Show less can cut it again. */
  less: boolean;
  copyText: string;
  refTitle: string;
  compareCmd: string;
  /** Its own session, where its calls can be stepped through, and what to call it there. */
  session: string | null;
  stepLabel: string;
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
  /** Every field a file tool's call asked for, for hover text; "" for a program. */
  hint: string;
  status: string;
  tone: Tone;
  meta: string;
  /** A closed run's error, first line. */
  preview: string;
  error: string;
  result: string;
  /** How much of a cut result or error shows; "" when it shows whole. */
  cut: string;
  /** Whether it shows whole after Show all, so Show less can cut it again. */
  less: boolean;
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
  /** What the run was, as its row says it: the agent's "program", or a file tool "call". */
  thing: "program" | "call";
}

export interface LogPane {
  count: string;
  rows: RunRow[];
  /** What the pane says when it has no rows, or `null`. */
  empty: string | null;
  runningNote: string;
  /** The run the running note is about, which Cancel stops. */
  runningRun: number | null;
  allLabel: string;
  /** The rows Expand all opens and closes. */
  ids: string[];
  allOpen: boolean;
}

/** How much of a long result or error the server shows unless asked for all of it. */
const SHOWN_BYTES = 4000;

/**
 * A result or error as it shows, and how much of it: "Shows 4 kB of 280 kB." when the
 * server cut it, or when it was read whole and the reader asked for less (`whole` false);
 * `less` when it shows whole and could be cut again.
 */
export function uncut(text: string, whole = false): { text: string; cut: string; less: boolean } {
  const m = /…\[(\d+) of (\d+) bytes shown; --include-payloads shows all\]$/.exec(text);
  if (m) return { text: text.slice(0, m.index) + "…", cut: "Shows " + size(Number(m[1])) + " of " + size(Number(m[2])) + ".", less: false };
  const bytes = new TextEncoder().encode(text).length;
  if (bytes <= SHOWN_BYTES) return { text, cut: "", less: false };
  if (whole) return { text, cut: "", less: true };
  return { text: text.slice(0, SHOWN_BYTES) + "…", cut: "Shows " + size(SHOWN_BYTES) + " of " + size(bytes) + ".", less: false };
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
  const what = fileTool(f, r.run) ? "call" : "program";
  // A program that never compiled fails the same way every time, so nothing is offered.
  if (word === "rejected") return [];
  const firstDenied = r.decisions.find((d) => d.outcome === "deny");
  const n = r.run;
  const options: AgainOption[] = [];
  if (firstDenied) {
    const ref = firstDenied.refs[0];
    // The refused check, not a ready answer: what to change is for you to tell your coding assistant.
    const text = decisionRef(f.names, r, firstDenied);
    options.push({
      action: { kind: "draft", decision: ref },
      goal: "Should the agent be allowed this?",
      what: "Change the blueprint first.",
      label: "Draft a rule for check " + ref,
      caution: false,
      copy: { label: "Copy for assistant", text, title: forAsst(text), what: "reference" },
      note: "Shows a rule that allows exactly the call refused in check " + ref + ". Nothing changes until you save it. If the refusal is right, there’s nothing to fix.",
    });
  }
  // A file tool's call has no program to test or rerun: its checks are decided again,
  // and the agent makes the call again itself.
  if (what === "call") {
    // A call that went through has nothing to check again: the agent got its answer.
    if (!firstDenied) return options;
    options.push({
      action: { kind: "recheck", run: n },
      goal: "Changed the blueprint?",
      what: "See whether this call gets through now.",
      label: f.inForce !== null ? "Check against blueprint v" + f.inForce : "Check against the blueprint in force",
      caution: false,
      copy: cliCopy("submilli playground recheck " + n),
      note: "Nothing runs. To make the call again, ask the agent.",
    });
    return options;
  }
  options.push(
    {
      action: { kind: "test", run: n },
      goal: "Changed the blueprint?",
      what: "See whether this " + what + " gets through now.",
      label: f.inForce !== null ? "Test against blueprint v" + f.inForce : "Test against the blueprint in force",
      caution: false,
      copy: cliCopy("submilli playground test " + n),
      note: "Nothing real happens: replies come from run " + n + "’s recording, and it keeps run " + n + "’s values.",
    },
    {
      action: { kind: "rerun", run: n },
      goal: "Changed the package, or want other variables?",
      what: "Run it for real.",
      label: "Rerun for real…",
      caution: true,
      copy: cliCopy("submilli playground rerun " + n),
      note: "Asks first. Uses the variables you’ve set now." + (firstDenied ? " A package change can’t lift a refusal; only the blueprint can." : ""),
    },
  );
  return options;
}

function cliCopy(cmd: string): AgainOption["copy"] {
  return { label: "Copy CLI command", text: cmd, title: "Copy the CLI command: " + cmd, what: "command" };
}

function decisionRow(f: Frame, r: Show, d: ShowDecision): DecisionRow {
  const ref = d.refs[0];
  const id = "d" + ref;
  const ex = f.data.explains[ref];
  const calls = (f.session.calls[r.run] || []).filter((c) => c.check && d.refs.includes(c.check.ref));
  const call = calls.find((c) => c.check?.ref === ref);
  // A repeated check names each line that asked, as the code pane's steps do.
  const lines = [...new Set(calls.map((c) => c.line).filter((l): l is number => !!l))];
  const bytes = calls.reduce((sum, c) => sum + (c.bytes || 0), 0);
  const spot = ruleSpot(f, r, d, ex);
  const copyText = decisionRef(f.names, r, d);
  // A repeated check asked for one thing each time; its first time stands for them.
  const asked = askedFor(r.untrusted.contexts?.[ref]);
  return {
    id,
    ref,
    selected: f.ui.sel === id,
    open: !!f.ui.open[id],
    title: " · " + d.caller + " → " + d.capability + (d.refs.length > 1 ? " ×" + d.refs.length : "") + (asked ? " " + asked : ""),
    hint: ctx(r.untrusted.contexts?.[ref]),
    status: d.outcome === "deny" ? "Denied" : "Allowed",
    tone: d.outcome === "deny" ? "bad" : "ok",
    meta: "blueprint v" + r.blueprint_version + (lines.length ? " · " + programLines(lines) + " · " + (bytes ? size(bytes) + " fetched" : "nothing fetched") : ""),
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

/** "program line 4", "program lines 4 and 9", or the first three and how many more. */
function programLines(lines: number[]): string {
  if (lines.length === 1) return "program line " + lines[0];
  if (lines.length === 2) return "program lines " + lines[0] + " and " + lines[1];
  return "program lines " + lines.slice(0, 3).join(", ") + (lines.length > 3 ? " (+" + (lines.length - 3) + " more)" : "");
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
  // A rerun uses the variables bound now, so a check it got past may owe that to them, not to a blueprint change.
  const otherVars = Object.fromEntries(Object.entries(fu.variables).filter(([k, v]) => r.variables[k] !== v));
  const byVars = !!fu.rerun_of && Object.keys(otherVars).length > 0;
  const byChange = fu.blueprint_version !== r.blueprint_version && !byVars;
  const allowed = fixed ? "check " + fixed.before + " is now allowed" + (fixed.rule ? " (rule " + fixed.rule.name + ", line " + fixed.rule.line + ")" : "") : "";
  const verdict = fixed
    ? byChange
      ? "Your change worked: " + allowed + "."
      : capitalize(allowed) + "."
    : w === word && cmp && !cmp.changes.length
      ? "Same result as run " + r.run + (refused ? ": refused at the same check, so nothing changed that check." : ".")
      : "";
  const further = !fixed
    ? ""
    : w === "stopped"
      ? "The program then went further than run " + r.run + " did. Its next call, below, has no recorded reply, so the test stopped rather than send it. Nothing was sent."
      : w === "completed"
        ? "The program then went further than run " + r.run + " did, and finished."
        : "The program then went further than run " + r.run + " did, but it " + (w === "denied" ? "was denied at a later check" : w === "cancelled" ? "was cancelled" : w === "rejected" ? "didn’t compile" : "failed") + "; its error is below.";
  const stopDetail = (ut.stop_detail || "").replace("the recorded run", "run " + r.run);
  const fuError = uncut(w === "stopped" ? "" : fu.untrusted.error || "", !!f.ui.whole[n]);
  const fuResult = uncut(fu.untrusted.result || "", !!f.ui.whole[n]);
  const copyText = runRef(f.names, fu);
  // A rerun takes the values bound when it ran; a test keeps the run's, whatever is bound now.
  const changed = fu.rerun_of ? differing(r.variables, fu.variables) : differing(fu.variables, f.data.status?.binding.variables ?? fu.variables);
  const values: ValuesNote | null = !changed.length
    ? null
    : fu.rerun_of
      ? { lead: "This rerun used other values than run " + r.run + ": ", changes: changed, tail: "." }
      : { lead: "This test kept run " + r.run + "’s values; the ones bound now differ: ", changes: changed, tail: ". Rerun for real uses the bound ones." };
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
    values,
    further,
    showFurther: !!further && w !== "stopped",
    what: fu.rerun_of
      ? "Run " + r.run + "’s program, run for real with the variables set at the time: " + boundVars(fu.variables) + "."
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
    error: fuError.text,
    result: fuResult.text,
    cut: [fuError.cut, fuResult.cut].filter(Boolean)[0] || "",
    less: fuError.less || fuResult.less,
    copyText,
    refTitle: forAsst(copyText),
    compareCmd: "submilli playground compare " + r.run + " " + n,
    session: fu.session,
    stepLabel: "Step through this " + (fu.rerun_of ? "rerun" : "test"),
    againHead: "Try run " + r.run + "’s " + (fileTool(f, r.run) ? "call" : "program") + " again",
    again,
  };
}

function runRow(f: Frame, r: Show, i: number, runs: Show[]): RunRow {
  const n = r.run;
  const id = "r" + n;
  const open = !!f.ui.open[id];
  const word = end(r.outcome);
  const again = againOptions(f, r, word);
  // The agent's own next program, not one your coding assistant or a file tool ran meanwhile.
  const later = byAgent(f, n) ? runs.slice(i + 1).find((x) => byAgent(f, x.run) && !fileTool(f, x.run)) : undefined;
  const againNote =
    word === "rejected" ? "This program didn’t compile, so running it again gives the same error." + (later ? " The agent’s next program is run " + later.run + "." : "") : "";
  const ups = f.stepping ? [] : followUps(f, n);
  const ascending = [...ups].sort((a, b) => a - b);
  // Drafting a rule is offered only under a follow-up that is still refused.
  const againAfter = (fu: Show) => (fu.decisions.some((d) => d.outcome === "deny") ? again : again.filter((o) => o.action.kind !== "draft"));
  const follow = ups.map((x) => followRow(f, r, f.data.runs[x], againAfter(f.data.runs[x]), ascending));
  const decisions = r.decisions.map((d) => decisionRow(f, r, d));
  const denials = r.decisions.filter((d) => d.outcome === "deny").length;
  const allowed = r.decisions.length - denials;
  const returned = f.session.returned[n];
  const err = uncut(r.untrusted.error || "", !!f.ui.whole[r.run]);
  const result = uncut(r.untrusted.result || "", !!f.ui.whole[r.run]);
  const copyText = runRef(f.names, r);
  const target = callTarget(f, n);
  return {
    n,
    hint: target.all,
    id,
    word,
    selected: f.ui.sel === id,
    open,
    thing: fileTool(f, n) ? "call" : "program",
    title: "Run " + n + " · " + (!byAgent(f, n) ? followUpName(f, n) ?? "run by your assistant" : fileTool(f, n) ? "the agent’s " + fileTool(f, n) + " call" + (target.text ? " " + target.text : "") : "the agent’s " + ordinal(programNumber(f, n)) + " program"),
    status: capitalize(word),
    tone: tone(word),
    meta:
      when(r.started_at) + " · " + duration(r.wall_ms) + " · blueprint v" + r.blueprint_version +
      (!byAgent(f, n) || word === "cancelled" ? "" : word === "completed" ? " · returned to agent " + size(returned || 0) : " · error sent to agent" + (returned ? ", " + size(returned) : "")),
    preview: !open && err.text ? err.text.split("\n")[0] : "",
    error: err.text,
    result: result.text,
    cut: [err.cut, result.cut].filter(Boolean)[0] || "",
    less: err.less || result.less,

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
  const allOpen = ids.length > 0 && ids.every((i) => f.ui.open[i]) && rows.every((x) => !x.follow.length || x.followOpen);
  // The program running at the cursor, or at the latest one running now that hasn't made a call yet.
  const runningRun = f.atCall ? f.atCall.run : f.running.length ? f.running[f.running.length - 1] : null;
  let empty: string | null = null;
  if (!rows.length && runningRun === null) {
    empty = f.ui.mode !== "open" || (!f.stepping && !runs.length) ? "No runs yet." : runs.length ? "Nothing matches this filter." : "Step forward to see runs.";
  }
  const whose = (n: number) => (byAgent(f, n) ? "the agent’s " + ordinal(programNumber(f, n)) + " program" : followUpName(f, n) ?? "run by your assistant");
  return {
    count: rows.length === all.length ? plural(rows.length, "run", "runs") : rows.length + " of " + plural(all.length, "run", "runs"),
    rows,
    empty,
    runningNote: runningRun !== null ? "Run " + runningRun + ", " + whose(runningRun) + ", is running. It appears here when it ends." : "",
    runningRun,
    allLabel: allOpen ? "Collapse all" : "Expand all",
    ids,
    allOpen,
  };
}

/** What Expand all (or Collapse all) changes: every row, with its checks and its reruns, opens or closes. */
export function expandAll(log: LogPane, ui: UiState): Pick<UiState, "open" | "checksOpen" | "followOpen"> {
  const open = { ...ui.open };
  const checksOpen = { ...ui.checksOpen };
  const followOpen = { ...ui.followOpen };
  for (const i of log.ids) open[i] = !log.allOpen;
  if (!log.allOpen) {
    for (const x of log.rows) {
      checksOpen[x.n] = true;
      for (const d of x.decisions) open[d.id] = true;
      if (x.follow.length) followOpen[x.n] = true;
      for (const fu of x.follow) open[fu.id] = true;
    }
  }
  return { open, checksOpen, followOpen };
}
