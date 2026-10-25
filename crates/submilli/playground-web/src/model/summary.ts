// The session summary: what was fetched, what went back to the agent, and the counts.
import { end, plural, size } from "./format";
import { byAgent, endedRuns, type Frame } from "./steps";

export interface Summary {
  fetched: string;
  returned: string;
  dataNote: string;
  execHead: string;
  execSub: string;
  checksHead: string;
  checksSub: string;
  /** A check was denied, so its count shows as a problem. */
  checksBad: boolean;
  followHead: string;
  followSub: string;
}

/** How many times each word occurs, in the order first seen: "2 completed · 1 denied". */
export function countWords(words: string[]): string {
  const counts = new Map<string, number>();
  for (const w of words) counts.set(w, (counts.get(w) || 0) + 1);
  return [...counts].map(([w, n]) => n + " " + w).join(" · ");
}

/** The tests and reruns of a run, newest first, whose answers are loaded. */
export function followUps(f: Frame, run: number): number[] {
  const show = f.data.runs[run];
  if (!show) return [];
  return show.reruns
    .map((x) => x.run)
    .filter((n) => f.data.runs[n])
    .sort((a, b) => b - a);
}

export function summary(f: Frame): Summary {
  if (f.ui.mode === "setup") {
    const dash = "—";
    return { fetched: dash, returned: dash, dataNote: "Start a session to record runs.", execHead: dash, execSub: "", checksHead: dash, checksSub: "", checksBad: false, followHead: dash, followSub: "" };
  }
  // Counted as each call is reached, so a step on a call already shows what it fetched. A
  // call no program line made has no step of its own, and counts at its run's end.
  const counted = new Set<string>();
  let fetched = 0;
  for (const s of f.vis) {
    if (s.kind === "call" || s.kind === "package") {
      const c = (f.session.calls[s.run] || []).find((x) => x.index === s.index);
      if (c && !counted.has(s.run + "." + c.index)) {
        counted.add(s.run + "." + c.index);
        fetched += c.bytes;
      }
    } else if (s.kind === "run") {
      for (const c of f.session.calls[s.run] || []) if (c.line === null) fetched += c.bytes;
    }
  }
  const runs = endedRuns(f);
  const words = runs.map((r) => end(r.outcome));
  // Only what went back to the agent: your coding assistant's runs answered you.
  const returned = runs.reduce((sum, r) => sum + (byAgent(f, r.run) && end(r.outcome) === "completed" ? f.session.returned[r.run] || 0 : 0), 0);
  // Only the agent's runs: the note says what the agent got instead of a result.
  const agentWords = runs.filter((r) => byAgent(f, r.run)).map((r) => end(r.outcome));
  const refused = agentWords.filter((w) => w === "denied").length;
  const failed = agentWords.filter((w) => w === "failed" || w === "rejected").length;
  const checks = runs.flatMap((r) => r.decisions);
  const denied = checks.filter((d) => d.outcome === "deny").length;
  const follows = f.stepping ? [] : runs.flatMap((r) => followUps(f, r.run));

  let dataNote = "";
  if (denied && !fetched) dataNote = "The call was denied before any data was fetched; the agent got the denial.";
  else if (fetched) dataNote = "Fetched data stayed in code; the agent got only each finished run’s result.";
  const notCounted = [refused ? plural(refused, "refused run", "refused runs") : "", failed ? plural(failed, "failed run", "failed runs") : ""]
    .filter(Boolean)
    .join(" and ");
  if (notCounted) {
    const omitted = "Returned to agent leaves out " + notCounted + ": the agent got the error message instead.";
    dataNote = dataNote ? dataNote + " " + omitted : omitted;
  }

  const allowed = checks.length - denied;
  const noChecks = checks.length === 0;

  return {
    fetched: size(fetched),
    returned: size(returned),
    dataNote: f.ui.mode === "fresh" ? "" : dataNote,
    execHead: plural(runs.length, "run", "runs"),
    execSub: countWords(words),
    checksHead: noChecks ? "0 checks" : allowed + " allowed",
    checksSub: noChecks ? "" : denied + " denied",
    checksBad: denied > 0,
    followHead: follows.length ? plural(follows.length, "run", "runs") : "None",
    followSub: follows.length ? "tests and live reruns" : "",
  };
}
