// The page's own lines: the waiting and failure states, the questions asked before a
// live run, and what a finished action says. Everything else the page shows comes from
// the wording in `../model`.
import type { BlueprintRefusal, Draft, Gap } from "../api/types";
import type { Action } from "../model/log";
import { boundVars, plural, short } from "../model/format";
import type { Names } from "../model/refs";

export const LOADING = "Loading the playground…";
export const NO_SESSIONS = "No sessions yet. Start one with New session.";
export const RECONNECTING = "Reconnecting to the session’s events…";
export const WAITING = "Waiting for the agent…";
export const SOURCE_LOADING = "Loading the package’s source…";
export const BP_LOADING = "Loading this version’s text…";
/** The notice for a session that started while another was shown: which one, and its first message when known. */
export function newSessionNotice(session: string, task?: string): string {
  const said = task ? (task.length > 60 ? task.slice(0, 59) + "…" : task) : "";
  return "Another session started while you were watching this one: " + short(session) + (said ? ", “" + said + "”" : "") + ".";
}
export const SESSION_LOADING = "Loading this session…";
export const NO_SESSIONS_CHAT = "Start a session with New session, or run `submilli playground chat` from the CLI. It shows here.";
/** Why the ask box waits: the agent is answering, here or in another session (its short id). */
export function answering(elsewhere: string | null): string {
  if (!elsewhere) return "The agent is still answering your last message.";
  return "The agent is still answering your last message, in " + (elsewhere === "new" ? "the session it started" : "session " + elsewhere.slice(0, 8)) + ".";
}

/** A run a link asked for that isn't recorded here. */
export function runMissing(run: number): string {
  return "Run " + run + " isn’t recorded in this playground.";
}

/** The blueprint file's last edit, which the playground refused, so the version before it stays. */
export function blueprintRefused(blueprint: string, refused: BlueprintRefusal, inForce: number | null): string {
  const where = refused.line ? " (line " + refused.line + ")" : "";
  const stays = inForce !== null ? " Version " + inForce + " stays in force, and runs use it." : " No version is in force.";
  return "Your last edit to blueprint " + blueprint + " wasn’t taken: " + refused.message + where + "." + stays;
}

/** A message the agent couldn't answer, where it was sent. */
export function chatFailed(message: string, started: boolean): { text: string; note: string } {
  // A provider that refused the request (a 4xx) refuses it again: only other settings help.
  // A rate limit (429) or a timeout (408) passes, so sending again can work.
  const again = !/\(HTTP 4(?!08|29)\d\d\)/.test(message);
  return {
    text: "The agent couldn’t answer: " + message.replace(/\.$/, "") + ".",
    note: started
      ? again
        ? "This session keeps its agent and settings. Send again, or start a New session to change them."
        : "This session keeps its agent and settings, which the provider refuses. Start a New session with others."
      : again
        ? "Nothing was started. Send again, or change the agent or its settings with New session."
        : "Nothing was started. Change the agent or its settings with New session.",
  };
}

/** A message whose answer the reader stopped, where it was sent. */
export function chatStopped(started: boolean): { text: string; note: string } {
  return {
    text: "You stopped the agent.",
    note: started
      ? "It stops after the step it’s on, and what it ran stays in the log. Send another message when you’re ready."
      : "Nothing was started. Send again when you’re ready.",
  };
}

export const SIGNED_OUT_WHY = "You’re signed out: the login link was used or has expired, or the playground restarted.";

/** Why the first load failed; the page tries again on its own. */
export function loadFailed(message: string): string {
  return message.replace(/\.$/, "") + ". This page tries again every few seconds.";
}

/** The feed's own error stays out: it's the server's words, not the reader's. */
export function feedStopped(): string {
  return "Live updates for this session stopped. Reload the page to follow it again.";
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
export function confirmOf(action: Action, bound: Record<string, string> = {}): { title: string; body: string; go: string } | null {
  if (action.kind === "rerun") {
    return {
      title: "Rerun run " + action.run + " for real?",
      body: "Every call goes out to the real services, including ones that change data. It uses the variables bound now: " + boundVars(bound) + ".",
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
