import { describe, expect, it } from "vitest";
import { askBox, chatEmptyText } from "./chat";
import { actionBlock, footer, sessionBar, sessionRows } from "./sessions";
import { frame } from "./steps";
import { chatSession, plainSession } from "./testdata";
import { INITIAL_UI } from "./ui";

describe("the sessions dialog", () => {
  it("says how a session's runs ended, in the log's words: denied, rejected, failed, cancelled", () => {
    const data = chatSession();
    const id = data.sessionId;
    data.sessions = data.sessions.map((s) => (s.session === id ? { ...s, denied: true, rejected: 1, failed: 2, cancelled: 1 } : s));
    const row = sessionRows(frame(data, INITIAL_UI)).find((r) => r.id === id)!;
    expect(row.ends).toEqual([
      { text: "denied", tone: "bad" },
      { text: "1 rejected", tone: "bad" },
      { text: "2 failed", tone: "bad" },
      { text: "1 cancelled", tone: "quiet" },
    ]);
  });

  it("lists only real sessions, newest first, each with its task as the server lists it", () => {
    const data = chatSession();
    data.sessions.push({ ...data.sessions[3], session: "5e550000-0000-4000-8000-000000000009", started_at: "2026-10-12T09:00:00Z", started_at_micros: 1, sources: ["mcp"], model: undefined, open: null, runs: [], run_count: 0, denied: false, cancelled: 0, rejected: 0, failed: 0, follow_ups: 0, asks: 0, untrusted: {} });
    const rows = sessionRows(frame(data, INITIAL_UI));
    expect(rows.map((r) => r.id)).toEqual(["5e550000-0000-4000-8000-000000000004", "5e550000-0000-4000-8000-000000000009"]);
    expect(rows[0]).toEqual({
      id: "5e550000-0000-4000-8000-000000000004",
      current: true,
      openable: false,
      task: "What's my total, and how many charges do I have? (+1 more)",
      taskMuted: false,
      short: "5e550000",
      copyText:
        "Submilli playground, session 5e550000 (details - project: `billing-demo`, see_with: `submilli playground runs --session 5e550000-0000-4000-8000-000000000004`)",
      started: "Oct 13, 13:26 UTC",
      source: "Chat",
      agent: "Claude · claude-sonnet-5-5",
      agentSettings: "",
      vars: ["customerId=cus_northwind"],
      runs: "4 runs",
      ends: [{ text: "denied", tone: "bad" }],
      follow: "3",
    });
    // A session with no chat says so; its row doesn't need its events loaded.
    expect(rows[1]).toMatchObject({ ends: [], task: "No chat", taskMuted: true, agent: "—", runs: "0 runs", follow: "", openable: true, current: false });
  });

  it("puts a new session not yet started first", () => {
    const fresh = { variables: { customerId: "cus_a" }, agent: "anthropic:claude-sonnet-5-5", settings: { max_output: 100 }, started: "2026-10-13T14:00:00Z", messages: [] };
    const rows = sessionRows(frame({ ...chatSession(), sessionId: null }, { ...INITIAL_UI, mode: "fresh", fresh }));
    expect(rows[0]).toMatchObject({ id: null, current: true, task: "No task yet", taskMuted: true, short: "not started", source: "Browser", agentSettings: "max output 100", vars: ["customerId=cus_a"], runs: "0 runs" });
  });
});

describe("the session bar", () => {
  it("shows the open chat session", () => {
    expect(sessionBar(frame(chatSession(), INITIAL_UI))).toEqual({
      short: "5e550000",
      copyText:
        "Submilli playground, session 5e550000 (details - project: `billing-demo`, see_with: `submilli playground runs --session 5e550000-0000-4000-8000-000000000004`)",
      vars: [{ name: "customerId", value: "cus_northwind", full: "customerId=cus_northwind" }],
      moreVars: "",
      allVars: [{ name: "customerId", value: "cus_northwind", full: "customerId=cus_northwind" }],
      settings: [],
      moreSettings: "",
      source: "Chat",
      agent: "Claude · claude-sonnet-5-5",
      started: "Started Oct 13, 13:26 UTC",
      state: "Open",
      live: true,
    });
  });

  it("opens every provider setting when the bar can't show them whole", () => {
    const data = chatSession();
    data.sessions[3].settings = { provider: { temperature: 0.2, top_p: 0.9 } };
    const bar = sessionBar(frame(data, INITIAL_UI))!;
    expect(bar.agent).toBe("Claude · claude-sonnet-5-5 · provider settings temperature=0.2 (+1 more)");
    expect(bar.moreSettings).toBe("All settings");
    expect(bar.settings).toEqual([
      { name: "provider: temperature", value: "0.2" },
      { name: "provider: top_p", value: "0.9" },
    ]);
  });

  it("shows two variables and a count when there are more than three", () => {
    const data = chatSession();
    data.sessions[3].variables = { a: "1", b: "2", c: "3", d: "4" };
    const bar = sessionBar(frame(data, INITIAL_UI))!;
    expect(bar.vars.map((v) => v.name)).toEqual(["a", "b"]);
    expect(bar.moreVars).toBe("+2 more");
    expect(bar.allVars.length).toBe(4);
  });

  it("an ended chat is Ended; an open one stepped back is still Open, but not live", () => {
    expect(sessionBar(frame(plainSession(), INITIAL_UI))).toMatchObject({ state: "Ended", live: false });
    expect(sessionBar(frame(chatSession(), { ...INITIAL_UI, cursor: "start" }))).toMatchObject({ state: "Open", live: false });
  });
});

describe("the footer and stepped-back actions", () => {
  it("says where the reader is", () => {
    expect(footer(frame(chatSession(), INITIAL_UI))).toBe("At the latest event");
    expect(footer(frame(chatSession(), { ...INITIAL_UI, cursor: "start" }))).toBe("Stepping through · read-only");
    expect(footer(frame({ ...chatSession(), events: chatSession().events.slice(0, 2) }, INITIAL_UI))).toBe("At the latest event");
    expect(footer(frame(chatSession(), { ...INITIAL_UI, mode: "setup" }))).toBe("New session · not started yet");
  });

  it("disables actions while stepped back, with the reason", () => {
    expect(actionBlock(frame(chatSession(), { ...INITIAL_UI, cursor: "start" }))).toBe("Back to latest to use this");
    expect(actionBlock(frame(chatSession(), INITIAL_UI))).toBeNull();
  });
});

describe("a session started for the coding assistant", () => {
  it("reads as open while it is, and says the agent doesn't chat in it", () => {
    const data = chatSession();
    data.sessions = data.sessions.map((s) => (s.session === data.sessionId ? { ...s, model: undefined, open: true, sources: ["assistant"] } : s));
    data.events = [];
    const f = frame(data, INITIAL_UI);
    expect(sessionBar(f)).toMatchObject({ state: "Open", live: true });
    expect(askBox(f)).toMatchObject({ disabled: true, none: { text: "No agent chats in this session.", offer: true } });
    expect(chatEmptyText(f)).toBe("No chat in this session: its runs came from your coding assistant or the CLI.");
  });
});
