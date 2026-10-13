// The page's own lines: the waiting and failure states, the questions asked before a
// live run, and what a finished action says. Everything else the page shows comes from
// the wording in `../model`.
import type { Draft, Gap } from "../api/types";
import type { Action } from "../model/log";
import { plural } from "../model/format";
import type { Names } from "../model/refs";

export const LOADING = "Loading the playground…";
export const NO_SESSIONS = "No sessions yet. Start one with New session.";
export const RECONNECTING = "Reconnecting to the session’s events…";
export const WAITING = "Waiting for the agent…";
export const SOURCE_LOADING = "Loading the package’s source…";
export const BP_LOADING = "Loading this version’s text…";
export const NEW_SESSION_NOTICE = "Another session started while you were watching this one.";

export function loadFailed(message: string): string {
  return "The playground didn’t answer: " + message + ".";
}

export function feedStopped(message: string): string {
  return "This session’s events stopped: " + message + ".";
}

/** A run whose events were dropped under load, in the gap's own terms. */
export function gapNote(run: number | undefined, gap: Gap): string {
  const whose = run !== undefined ? "run " + run + "’s" : "a run’s";
  let text =
    gap.dropped === null
      ? "Some of " + whose + " events weren’t recorded, including how it ended."
      : "Some of " + whose + " events weren’t recorded: " + plural(gap.dropped, "event was", "events were") + " dropped.";
  if (gap.recovered) text += " " + plural(gap.recovered, "was", "were") + " recovered from the run’s record.";
  if (gap.lost) text += " The run’s record was cut too, so some of its checks or calls are gone.";
  return text;
}

/** What the page asks before a test or rerun that sends real calls. */
export function confirmOf(action: Action): { title: string; body: string; go: string } | null {
  if (action.kind === "rerun") {
    return {
      title: "Rerun run " + action.run + " for real?",
      body: "Every call goes out to the real services, including ones that change data. It uses the variables you’ve set now.",
      go: "Rerun for real",
    };
  }
  if (action.kind === "test" && action.mode === "live") {
    return {
      title: "Test run " + action.run + " live?",
      body: "Every call goes out, including ones that change data.",
      go: "Test it live",
    };
  }
  return null;
}

/** The same action from the CLI. */
export function commandOf(action: Action): string {
  if (action.kind === "draft") return "submilli playground draft-rule " + action.decision;
  if (action.kind === "rerun") return "submilli playground rerun " + action.run;
  return "submilli playground test " + action.run + (action.mode ? " --" + action.mode : "");
}

export function followUpDone(source: number, run: number, word: string): string {
  return "Run " + run + " " + word + ". It’s under run " + source + ", in Reruns by you.";
}

/** Where a drafted rule would go, from the draft's own answer. */
export function draftWhere(draft: Draft): string {
  return "It would go in " + draft.file_in_project + ", lines " + draft.lines[0] + "–" + draft.lines[1] + ".";
}

export const DRAFT_NOTE = "Nothing is written: the blueprint stays as it is until you or your coding assistant edit it.";

export function draftCommand(draft: Draft): string {
  return "submilli playground draft-rule " + draft.decision + " --write";
}

export function draftRef(names: Names, draft: Draft): string {
  return (
    "Submilli playground draft of a rule allowing decision " + draft.decision + " in project " + names.project +
    ", for " + draft.file_in_project + " lines " + draft.lines[0] + "-" + draft.lines[1] + ". Print it: submilli playground draft-rule " + draft.decision
  );
}

export function notInForce(inForce: number): string {
  return "Not the version in force: v" + inForce + " is in force now.";
}
