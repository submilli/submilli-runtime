import { describe, expect, it } from "vitest";
import { askBox, chatEmptyText, chatItems } from "./chat";
import { logPane } from "./log";
import { frame, readSession } from "./steps";
import { chatSession, plainSession } from "./testdata";
import { INITIAL_UI, type UiState } from "./ui";

const data = chatSession();
const { steps } = readSession(data.events);
const atStep = (k: number, over: Partial<UiState> = {}) => frame(data, { ...INITIAL_UI, cursor: steps[k - 1].key, ...over });

describe("the chat while stepping", () => {
  it("says a program is running until its run ends, and the log says so too", () => {
    for (const k of [3, 4, 5]) {
      const f = atStep(k);
      const chat = chatItems(f);
      expect(chat[chat.length - 1]).toEqual({ kind: "running", text: "Running program 1…" });
      // Only once, however many of its calls have been reached.
      expect(chat.filter((c) => c.kind === "running").length).toBe(1);
      const log = logPane(f);
      expect(log.runningNote).toBe("Run 1, the agent’s 1st program, is running. It appears here when it ends.");
      expect(log.empty).toBeNull();
    }
  });

  it("puts the run's chip in once the run ends", () => {
    const f = atStep(6);
    const chat = chatItems(f);
    expect(chat[chat.length - 1]).toEqual({ kind: "run", text: "Ran program 1 · completed →", run: 1, selected: false });
    expect(logPane(f).count).toBe("1 run");
    expect(logPane(f).runningNote).toBe("");
  });

  it("before any run, the log says to step forward", () => {
    const f = atStep(2);
    const chat = chatItems(f);
    expect(chat[chat.length - 1]).toEqual({ kind: "message", who: "Agent", role: "agent", text: "Let me total your charges." });
    expect(logPane(f).empty).toBe("Step forward to see runs.");
  });

  it("from the start is empty", () => {
    const f = frame(data, { ...INITIAL_UI, cursor: "start" });
    expect(chatItems(f)).toEqual([]);
    expect(chatEmptyText(f)).toBe("Step forward to see the task.");
  });
});

describe("the chat at the latest event", () => {
  it("lists the messages and a chip per run, without the agent's execute calls", () => {
    const chat = chatItems(frame(data, { ...INITIAL_UI, sel: "r2" }));
    expect(chat.map((c) => c.text)).toEqual([
      "What's my total, and how many charges do I have?",
      "Let me total your charges.",
      "Ran program 1 · completed →",
      "Checking another account too.",
      "Ran program 2 · denied →",
      "One more.",
      "Ran program 3 · rejected →",
      "You have the charges listed; the total is in cents. I couldn't read cus_globex: that was denied.",
      "Which are the largest three?",
      "Here are your top three.",
      "Ran program 4 · completed →",
      "Your three largest charges are listed above.",
    ]);
    expect(chat.find((c) => c.kind === "run" && c.run === 2)).toMatchObject({ selected: true });
  });

  it("shows the agent's other tools with their friendly names", () => {
    const chat = chatItems(frame(plainSession(), INITIAL_UI));
    expect(chat.find((c) => c.kind === "tool")).toEqual({ kind: "tool", text: "Searched packages · 2 kB back to the agent" });
  });

  it("an open chat session takes a reply; a stepped-back or ended one doesn't", () => {
    expect(askBox(frame(data, INITIAL_UI))).toEqual({ placeholder: "Reply in this session…", disabled: false, primary: true });
    expect(askBox(atStep(3))).toMatchObject({ disabled: true, reason: "Back to latest to use this" });
    expect(askBox(frame(plainSession(), INITIAL_UI))).toEqual({
      placeholder: "This session has ended. Start a new one to chat.",
      disabled: true,
      primary: false,
    });
  });

  it("a new session's messages show while the agent answers", () => {
    const fresh = { variables: { customerId: "cus_northwind" }, agent: "anthropic:claude-sonnet-5-5", settings: null, started: "2026-10-13T14:00:00Z", messages: ["Hi"] };
    const f = frame({ ...data, sessionId: null, events: [] }, { ...INITIAL_UI, mode: "fresh", fresh });
    expect(chatItems(f)).toEqual([
      { kind: "message", who: "You", role: "user", text: "Hi" },
      { kind: "wait", text: "Waiting for the agent…" },
    ]);
    expect(askBox(f)).toEqual({ placeholder: "Ask the agent to do a task…", disabled: false, primary: true });
    const none = frame({ ...data, sessionId: null, events: [] }, { ...INITIAL_UI, mode: "fresh", fresh: { ...fresh, messages: [] } });
    expect(chatEmptyText(none)).toBe("Session started. Ask the agent to do a task.");
    expect(chatEmptyText(frame(data, { ...INITIAL_UI, mode: "setup" }))).toBe("Set the session’s variables, then start a session to chat.");
  });
});
