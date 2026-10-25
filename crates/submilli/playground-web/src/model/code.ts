// The code pane: which program or package source shows, which line is lit, and what the
// step at the cursor did there.
import { sourceKey, type Show } from "../api/types";
import { capitalize, size, sourceRunOf } from "./format";
import { programRef } from "./refs";
import { selectedRun } from "./chat";
import { byAgent, endedRuns, fileTool, followUpName, programIdentity, programNumber, type Call, type Frame } from "./steps";

/** "program 2", or the run your coding assistant made. */
function programName(f: Frame, run: number): string {
  const tool = fileTool(f, run);
  if (tool) return "the agent’s " + tool + " call";
  // A run of another session, as a rerun's source is from inside the rerun's session, has no number here.
  if (byAgent(f, run)) return programNumber(f, run) ? "program " + programNumber(f, run) : "the agent’s program";
  const followUp = followUpName(f, run);
  return followUp ? "the program of " + followUp : "your assistant’s program";
}

export interface CodeLine {
  num: number;
  text: string;
  active: boolean;
}

export interface CodePane {
  /** The pane's subtitle: which program, or which package and where it was called from. */
  sub: string;
  lines: CodeLine[];
  /** A package's source shows, rather than a program. */
  isPackage: boolean;
  codeNote: string;
  /** What the step at the cursor did on the lit line. */
  stepNote: string;
  /** What the pane says when there are no lines. */
  empty: string | null;
  /** The copy-for-assistant line for the program shown. */
  programRef: string | null;
}

/** The run whose program a test or rerun ran: the program is the source run's. */
function sourceOf(f: Frame, run: number): number {
  const r = f.data.runs[run];
  return (r && sourceRunOf(r)) ?? run;
}

/**
 * A check as the log finds it: the log lists repeats of one call as one row, under the
 * first check's number, so a later one says which row it is in.
 */
function checkName(f: Frame, run: number, ref: string): string {
  const group = f.data.runs[run]?.decisions.find((d) => d.refs.includes(ref));
  return group && group.refs[0] !== ref ? "check " + ref + "; the log lists it under check " + group.refs[0] + ", ×" + group.refs.length : "check " + ref;
}

// "main → pied-piper.example/charges.list, allowed by the blueprint (check 1.1)", or just who called what.
function callWhat(f: Frame, c: Call): string {
  const k = c.check;
  return k ? k.caller + " → " + k.capability + ", " + (k.allowed ? "allowed" : "denied") + " by the blueprint (" + checkName(f, c.run, k.ref) + ")" : c.caller + " → " + c.capability;
}

function fetched(c: Call): string {
  return c.bytes ? size(c.bytes) + " fetched" : "";
}

/** The note for a step on a call, in one of three forms: a program line making a call, one calling into a package, or a line inside the package. */
function stepNoteOf(f: Frame, c: Call, inPackage: boolean): string {
  const pkg = c.pkg;
  if (inPackage && pkg) {
    const where = "Inside " + pkg.package + (pkg.function ? ", " + pkg.function + "()" : "") + ", line " + pkg.line;
    const k = c.check;
    const verdict = k ? (k.allowed ? "Allowed" : "Denied") + " (" + checkName(f, c.run, k.ref) + ")" : "";
    // A package's check asks on the program's behalf; its own calls answer to its own rules.
    if (k && k.caller !== pkg.package) {
      return where + " asks the blueprint whether the program may use " + k.capability + ". " + verdict + ".";
    }
    const tail = [verdict, fetched(c)].filter(Boolean).join(", ");
    return where + " makes the package’s own call, " + c.capability + ", under the blueprint’s rules for " + pkg.package + "." + (tail ? " " + capitalize(tail) + "." : "");
  }
  if (pkg) {
    return "Line " + c.line + " calls into " + pkg.package + (pkg.function ? " (" + pkg.function + ")" : "") + ". Step → to follow it into the package’s code.";
  }
  const bytes = fetched(c);
  return "Line " + c.line + " made a call: " + callWhat(f, c) + (bytes ? ", " + bytes : "") + ".";
}

function linesOf(text: string, active: number | null): CodeLine[] {
  return text.split("\n").map((t, i) => ({ num: i + 1, text: t, active: active === i + 1 }));
}

export function codePane(f: Frame): CodePane {
  const { ui, data } = f;
  const blank = { codeNote: "", stepNote: "", programRef: null, isPackage: false };
  if (ui.mode !== "open") return { ...blank, sub: "", lines: [], empty: "The programs the agent sends appear here." };

  // While stepping, the code shows the program at the step, unless a run was picked since.
  const programs = endedRuns(f).map((r) => r.run);
  // A run still going shows its program as soon as its start is read, ahead of the ones before it.
  for (const n of f.running) if (data.running[n] !== undefined && !programs.includes(n)) programs.push(n);
  const atCall = f.atCall;
  if (atCall && !programs.includes(atCall.run)) programs.push(atCall.run);
  programs.sort((a, b) => a - b);
  const picked = selectedRun(ui.sel);
  let shown: number | null = ui.program ?? picked ?? (atCall ? atCall.run : null);
  if (shown !== null && !programs.includes(sourceOf(f, shown)) && picked === null && ui.program === null) shown = null;
  if (shown === null && programs.length) shown = programs[programs.length - 1];
  const codeRun: Show | undefined = shown !== null ? data.runs[sourceOf(f, shown)] : undefined;

  let active: number | null = null;
  if (ui.sel && ui.sel.startsWith("d") && picked !== null) {
    // A link to a repeated check lights the line of the one it names, not the first.
    const ref = ui.asked && ui.asked.sel === ui.sel ? ui.asked.ref : ui.sel.slice(1);
    const c = (f.session.calls[picked] || []).find((x) => x.check?.ref === ref);
    active = c ? c.line : null;
  }
  if (ui.sel && ui.sel.startsWith("f") && picked !== null) active = data.runs[picked]?.test?.stopped?.line ?? null;

  // The step at the cursor, when the program it's in is the one shown.
  if (atCall && shown === atCall.run && !ui.sel) {
    const c = (f.session.calls[atCall.run] || []).find((x) => x.index === atCall.index);
    if (c && atCall.kind === "package" && c.pkg) {
      const pkg = c.pkg;
      const src = data.sources[sourceKey(pkg.package, pkg.module)];
      const cut = src && src.truncated && src.note ? " " + capitalize(src.note) + "." : "";
      return {
        sub: pkg.package + " · " + pkg.module + " · called from " + programName(f, atCall.run) + ", line " + c.line,
        lines: src ? linesOf(src.untrusted.text, pkg.line) : [],
        isPackage: true,
        // The page serves the package as installed now, not the build a past run used.
        codeNote: src ? "The package’s source as installed now, which may differ from the build this run used." + cut : "",
        stepNote: stepNoteOf(f, c, true),
        empty: src ? null : pkg.package + " · " + pkg.module + ": its source isn’t available.",
        programRef: null,
      };
    }
    if (c) {
      active = c.line;
      blank.stepNote = stepNoteOf(f, c, false);
    }
  }

  const inFlight = !codeRun && shown !== null ? data.running[shown] : undefined;
  if (shown !== null && inFlight !== undefined) {
    return {
      ...blank,
      sub: programIdentity(f, shown) + " · Running",
      lines: linesOf(inFlight, active),
      codeNote: "Still running: its result shows in the log when it ends.",
      empty: null,
      programRef: programRef(f.names, shown),
    };
  }
  if (!codeRun) {
    if (picked !== null && !data.running[picked]) return { ...blank, sub: "run " + picked, lines: [], empty: "Source was not recorded for this run." };
    // A run whose start hasn't been read yet: its program shows once it is.
    const running = atCall ? atCall.run : f.running.length ? f.running[f.running.length - 1] : null;
    if (running !== null) return { ...blank, sub: "run " + running, lines: [], empty: "Run " + running + " is running; its program shows here as soon as it’s read, or when it ends." };
    // Stepping forward only helps a reader who is stepped back.
    const empty = f.stepping ? "No program yet at this point. Step forward (→) to reach the agent’s first program." : "The programs the agent sends appear here.";
    return { ...blank, sub: "", lines: [], empty };
  }
  const text = codeRun.untrusted.program;
  return {
    ...blank,
    sub: (shown !== null && shown !== codeRun.run ? "Source: " : "") + programIdentity(f, codeRun.run),
    lines: text ? linesOf(text, active) : [],
    codeNote: shown !== null && shown !== codeRun.run ? "Used by Run " + shown + ". This is the original program from Run " + codeRun.run + "." : "",
    empty: text ? null : fileTool(f, codeRun.run) ? "Run " + codeRun.run + " was the agent’s " + fileTool(f, codeRun.run) + " call, not a program." : "Run " + codeRun.run + "’s program wasn’t recorded.",
    programRef: programRef(f.names, codeRun.run),
  };
}
