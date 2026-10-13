import { describe, expect, it } from "vitest";
import { actionBlock, footer, sessionBar, sessionRows } from "./sessions";
import { frame } from "./steps";
import { chatSession, plainSession } from "./testdata";
import { INITIAL_UI } from "./ui";

describe("the sessions dialog", () => {
  it("lists only real sessions, newest first, with the shown one's task", () => {
    const data = chatSession();
    data.sessions.push({ ...data.sessions[3], session: "5e550000-0000-4000-8000-000000000009", started_at: "2026-10-12T09:00:00Z", started_at_micros: 1, sources: ["mcp"], model: undefined, open: null, runs: [], run_count: 0, denied: false });
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
        "Submilli playground session 5e550000-0000-4000-8000-000000000004 in project billing-demo. Its runs: submilli playground runs --session 5e550000-0000-4000-8000-000000000004",
      started: "Oct 13, 13:26 UTC",
      source: "Chat",
      agent: "Claude · claude-sonnet-5-5",
      agentSettings: "",
      vars: ["customerId=cus_northwind"],
      runs: "4 runs",
      denied: true,
      follow: "3",
    });
    // Another session's events aren't loaded, so its task isn't known yet.
    expect(rows[1]).toMatchObject({ task: "No chat loaded", taskMuted: true, agent: "—", runs: "0 runs", follow: "", openable: true, current: false });
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
        "Submilli playground session 5e550000-0000-4000-8000-000000000004 in project billing-demo. Its runs: submilli playground runs --session 5e550000-0000-4000-8000-000000000004",
      vars: [{ name: "customerId", value: "cus_northwind", full: "customerId=cus_northwind" }],
      moreVars: "",
      allVars: [{ name: "customerId", value: "cus_northwind", full: "customerId=cus_northwind" }],
      source: "Chat",
      agent: "Claude · claude-sonnet-5-5",
      started: "Started Oct 13, 13:26 UTC",
      state: "Open",
      live: true,
    });
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
    expect(footer(frame({ ...chatSession(), events: chatSession().events.slice(0, 2) }, INITIAL_UI))).toBe("Live session · waiting for a run");
    expect(footer(frame(chatSession(), { ...INITIAL_UI, mode: "setup" }))).toBe("New session · not started yet");
  });

  it("disables actions while stepped back, with the reason", () => {
    expect(actionBlock(frame(chatSession(), { ...INITIAL_UI, cursor: "start" }))).toBe("Back to latest to use this");
    expect(actionBlock(frame(chatSession(), INITIAL_UI))).toBeNull();
  });
});
