// The chat pane: the conversation up to the cursor, with a line for each run.
import { end, size } from "./format";
import { actionBlock } from "./sessions";
import { programNumber, type Frame } from "./steps";

export type ChatItem =
  | { kind: "message"; who: "You" | "Agent"; role: "user" | "agent"; text: string }
  /** One of the agent's other tools, such as a package search. */
  | { kind: "tool"; text: string }
  /** A program still running at this step: its run hasn't ended yet. */
  | { kind: "running"; text: string }
  /** A run's chip, which picks the run in the log. */
  | { kind: "run"; text: string; run: number; selected: boolean }
  /** A new session's first message, sent and not answered yet. */
  | { kind: "wait"; text: string };

/** The agent's tools other than running a program, by the name they're called with. */
const TOOLS: Record<string, string> = { search: "Searched packages", docs: "Read package docs" };

/** The run a log selection belongs to: `r2`, `d2.1` and `f5` give 2, 2 and 5. */
export function selectedRun(sel: string | null): number | null {
  return sel ? Number(sel.slice(1).split(".")[0]) : null;
}

/** How a run ended, from its answer, or from its last event while the answer loads. */
export function runWord(f: Frame, run: number): string {
  const show = f.data.runs[run];
  if (show) return end(show.outcome);
  const error = f.session.finished[run]?.error;
  return error ? end({ kind: "failed", error }) : "completed";
}

export function chatItems(f: Frame): ChatItem[] {
  if (f.ui.mode === "fresh" && f.ui.fresh) {
    const sent = f.ui.fresh.messages;
    const items: ChatItem[] = sent.map((text) => ({ kind: "message", who: "You", role: "user", text }));
    if (sent.length) items.push({ kind: "wait", text: "Waiting for the agent…" });
    return items;
  }
  const items: ChatItem[] = [];
  const running = new Set<number>();
  const picked = selectedRun(f.ui.sel);
  for (const s of f.vis) {
    if (s.kind === "chat") {
      const user = s.role === "user";
      items.push({ kind: "message", who: user ? "You" : "Agent", role: user ? "user" : "agent", text: s.text });
    } else if (s.kind === "tool") {
      items.push({ kind: "tool", text: (TOOLS[s.tool] || s.tool) + " · " + size(s.bytes) + " back to the agent" });
    } else if (s.kind === "run") {
      items.push({ kind: "run", text: "Ran program " + programNumber(f, s.run) + " · " + runWord(f, s.run) + " →", run: s.run, selected: picked === s.run });
    } else if (!f.ended.includes(s.run) && !running.has(s.run)) {
      running.add(s.run);
      items.push({ kind: "running", text: "Running program " + programNumber(f, s.run) + "…" });
    }
  }
  return items;
}

export function chatEmptyText(f: Frame): string {
  if (f.ui.mode === "setup") return "Set the session’s variables, then start a session to chat.";
  if (f.ui.mode === "fresh") return "Session started. Ask the agent to do a task.";
  return "Step forward to see the task.";
}

export interface AskBox {
  placeholder: string;
  disabled: boolean;
  /** Send is the page's main action when a message can go. */
  primary: boolean;
  /** Why it can't be used, when that's because the reader stepped back. */
  reason?: string;
}

/** Whether a message can be sent: a new session takes its first; an open chat session takes more. */
export function askBox(f: Frame): AskBox {
  if (f.ui.mode === "fresh") return { placeholder: "Ask the agent to do a task…", disabled: false, primary: true };
  if (f.ui.mode === "setup") return { placeholder: "Start a session to chat", disabled: true, primary: false };
  const open = !!(f.info && f.info.model && f.info.open);
  const blocked = actionBlock(f);
  if (open && blocked) return { placeholder: "Reply in this session…", disabled: true, primary: false, reason: blocked };
  if (open) return { placeholder: "Reply in this session…", disabled: false, primary: true };
  return { placeholder: "This session has ended. Start a new one to chat.", disabled: true, primary: false };
}
