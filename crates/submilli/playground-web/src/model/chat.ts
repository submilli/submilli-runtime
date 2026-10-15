// The chat pane: the conversation up to the cursor, with a line for each run.
import { askedFor, ctx, end, size, sourceRunOf } from "./format";
import { actionBlock } from "./sessions";
import { byAgent, callTarget, fileTool, programNumber, type Frame } from "./steps";

export type ChatItem =
  | { kind: "message"; who: "You" | "Agent"; role: "user" | "agent"; text: string }
  /** One of the agent's other tools, such as a package search. */
  | { kind: "tool"; text: string; hint?: string }
  /** A program still running at this step: its run hasn't ended yet. */
  | { kind: "running"; text: string }
  /** A run's chip, which picks the run in the log. */
  /** `hint` is every field the call asked for, for hover text. */
  | { kind: "run"; text: string; run: number; selected: boolean; hint?: string }
  /** A new session's first message, sent and not answered yet. */
  | { kind: "wait"; text: string }
  /** A message the agent couldn't answer, and why. */
  | { kind: "failed"; text: string; note: string }
  /** A message whose answer was stopped from the page. */
  | { kind: "stopped"; text: string; note: string };

/** The agent's file tools, by the name they're called with. */
const FILE_TOOLS = new Set(["list", "list_files", "read", "read_file"]);

const TOOLS: Record<string, string> = {
  search: "Searched packages",
  docs: "Read package docs",
  // The run it made says whether the blueprint let it.
  list: "Asked to list project files",
  list_files: "Asked to list project files",
  read: "Asked to read a project file",
  read_file: "Asked to read a project file",
};

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
      // A file tool's own run names what it asked on its chip; the others say it here.
      const asked = FILE_TOOLS.has(s.tool) ? "" : askedFor(s.args);
      // A file tool's answer is its run's, which Returned to agent counts; a lookup's isn't
      // the app's data, so it isn't said the same way.
      const sent = FILE_TOOLS.has(s.tool) ? " back to the agent" : " answer";
      const tool: ChatItem = { kind: "tool", text: (TOOLS[s.tool] || s.tool) + (asked ? " for " + asked : "") + " · " + size(s.bytes) + sent };
      if (asked) tool.hint = ctx(s.args);
      items.push(tool);
    } else if (!byAgent(f, s.run)) {
      // Your coding assistant's runs show in the log; the agent never saw them.
      continue;
    } else if (s.kind === "run") {
      const tool = fileTool(f, s.run);
      const target = callTarget(f, s.run);
      const what = tool ? "Called " + tool + (target.text ? " " + target.text : "") : "Ran program " + programNumber(f, s.run);
      const chip: ChatItem = { kind: "run", text: what + " · " + runWord(f, s.run) + " →", run: s.run, selected: picked === s.run };
      if (target.all) chip.hint = target.all;
      items.push(chip);
    } else if (!f.ended.includes(s.run) && !running.has(s.run)) {
      running.add(s.run);
      items.push({ kind: "running", text: "Running program " + programNumber(f, s.run) + "…" });
    }
  }
  // A program running now that hasn't made a call yet has no step of its own.
  for (const n of f.running) {
    if (running.has(n) || !byAgent(f, n)) continue;
    items.push({ kind: "running", text: "Running program " + programNumber(f, n) + "…" });
  }
  return items;
}

/** In a test's or rerun's own session: which it is, and of which run, when known. */
export function followUpSession(f: Frame, knownSource: number | null = null): { kind: "test" | "rerun"; run: number | null; source: number | null } | null {
  if (f.ui.mode !== "open" || !f.info) return null;
  const kind = f.info.sources.includes("rerun") ? "rerun" : f.info.sources.includes("test") ? "test" : null;
  if (!kind) return null;
  const run = f.info.runs[0] ?? f.running[0] ?? null;
  const show = run !== null ? f.data.runs[run] : undefined;
  // While it runs there is no record yet; the run it followed may be known from where it was started.
  return { kind, run, source: (show ? sourceRunOf(show) : null) ?? knownSource };
}

export function chatEmptyText(f: Frame, knownSource: number | null = null): string {
  if (f.ui.mode === "setup") return "Set the session’s variables, then start a session to chat.";
  if (f.ui.mode === "fresh") return "Session started. Ask the agent to do a task.";
  const fu = followUpSession(f, knownSource);
  if (fu) {
    const what = (fu.run !== null ? "run " + fu.run + ", " : "") + "a " + fu.kind + (fu.source !== null ? " of run " + fu.source : "");
    return "No chat: this session is " + what + " that you started. The agent never saw it. Step through its calls with ← →.";
  }
  // At the latest event with nothing said, there is no task to step to.
  if (!f.stepping && f.ui.mode === "open") return f.info?.model ? "No messages in this session yet." : "No chat in this session: its runs came from your coding assistant or the CLI.";
  return "Step forward to see the task.";
}

export interface AskBox {
  placeholder: string;
  disabled: boolean;
  /** Send is the page's main action when a message can go. */
  primary: boolean;
  /** Why it can't be used, when that's because the reader stepped back. */
  reason?: string;
  /** The agent is answering a message sent here: Send becomes Stop. */
  stop?: boolean;
  /** It can't send yet, but the next message can be written in it meanwhile. */
  typeAhead?: boolean;
  /** When there's no chat to send into: why, shown in the box's place, and whether New session is offered there. */
  none?: { text: string; offer: boolean; /** Back to the session the reader came from, by its short id. */ back?: string };
}

/** Whether a message can be sent: a new session takes its first; an open chat session takes more. */
/** No box to type in: a line saying why, and New session where it's the way on. */
export function noChat(text: string, offer: boolean): AskBox {
  return { placeholder: "", disabled: true, primary: false, none: { text, offer } };
}

export function askBox(f: Frame): AskBox {
  if (f.ui.mode === "fresh") return { placeholder: "Ask the agent to do a task…", disabled: false, primary: true };
  // The form is open above, so its Start session is the way on.
  if (f.ui.mode === "setup") return noChat("Start the session above to chat.", false);
  const fu = followUpSession(f);
  if (fu) return noChat("A " + fu.kind + " takes no messages.", false);
  // Its conversation is in Claude Code, not here.
  if (f.info?.sources.includes("stand-in") && !f.info.model) return noChat("You chat with this session’s stand-in in Claude Code.", true);
  const open = !!(f.info && f.info.model && f.info.open);
  const blocked = actionBlock(f);
  if (open && blocked) return { placeholder: "Reply in this session…", disabled: true, primary: false, reason: blocked };
  if (open) return { placeholder: "Reply in this session…", disabled: false, primary: true };
  if (f.info?.open) return noChat("No agent chats in this session.", true);
  return noChat(f.info ? "This session has ended." : "No session yet.", true);
}
