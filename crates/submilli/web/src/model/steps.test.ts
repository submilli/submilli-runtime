import { describe, expect, it } from "vitest";
import type { StoredEvent } from "../api/types";
import { AT_START, nextKey, playFrom, prevKey, readSession, reached, stepBar, type Step } from "./steps";
import { chatSession, feed, plainSession } from "./testdata";

// One short label per step: what it is, and for a call which run and call index.
function label(s: Step): string {
  switch (s.kind) {
    case "chat":
      return `${s.role}: ${s.text}`;
    case "tool":
      return `tool ${s.tool}`;
    case "call":
      return `call ${s.run}.${s.index}${s.into ? " into package" : ""}`;
    case "package":
      return `package ${s.run}.${s.index}`;
    case "run":
      return `run ${s.run}`;
  }
}

describe("the step list", () => {
  it("puts a package call's program line before its package line, once per program line", () => {
    const { steps } = readSession(chatSession().events);
    expect(steps.map(label)).toEqual([
      "user: What's my total, and how many charges do I have?",
      "assistant: Let me total your charges.",
      "call 1.0 into package",
      "package 1.0",
      // The package's own read is on the same program line, so the program stays put.
      "package 1.1",
      "run 1",
      "assistant: Checking another account too.",
      "call 2.0 into package",
      "package 2.0",
      "run 2",
      "assistant: One more.",
      "run 3",
      "assistant: You have the charges listed; the total is in cents. I couldn't read cus_globex: that was denied.",
      "user: Which are the largest three?",
      "assistant: Here are your top three.",
      "call 4.0 into package",
      "package 4.0",
      "package 4.1",
      "run 4",
      "assistant: Your three largest charges are listed above.",
    ]);
  });

  it("gives a call with no package line one step, and a call with no line none", () => {
    const { steps, calls } = readSession(plainSession().events);
    expect(steps.map(label)).toEqual([
      "user: Is the status service up?",
      // The copy that replaced the live message, where it was placed.
      "assistant: Checking the status page.",
      "tool search",
      "call 10.0",
      "run 10",
      "assistant: The status service is up.",
    ]);
    expect(calls[10].map((c) => [c.line, c.bytes, c.check?.ref])).toEqual([
      [3, 1200, "10.1"],
      [null, 500, "10.2"],
    ]);
  });

  it("keys steps by event, by event plus :pkg for a package line, and chat by turn", () => {
    const { steps } = readSession(chatSession().events);
    expect(steps.slice(0, 5).map((s) => s.key)).toEqual([
      "turn:chat-0",
      "turn:chat-1",
      "ev:00000000-0000-4000-8000-000000000006-2",
      "ev:00000000-0000-4000-8000-000000000006-2:pkg",
      "ev:00000000-0000-4000-8000-000000000006-5:pkg",
    ]);
  });

  it("reads each run's calls, their bytes, and which decision belongs to which call", () => {
    const { calls, returned } = readSession(chatSession().events);
    expect(calls[1].map((c) => [c.caller, c.capability, c.line, c.pkg?.line, c.outcome, c.bytes, c.check?.ref])).toEqual([
      ["main", "acme.com/charges.list", 4, 28, "returned", 0, "1.1"],
      ["@acme/billing", "fs.read", 4, 31, "returned", 21530, "1.2"],
    ]);
    expect(returned).toEqual({ 1: 64, 2: 1061, 3: 361, 4: 103 });
  });
});

describe("the cursor", () => {
  const { steps } = readSession(chatSession().events);

  it("at the latest step follows live and shows every step", () => {
    expect(reached(steps, null)).toBe(20);
    expect(stepBar(steps, null)).toEqual({ position: "20 / 20", atStart: false, atEnd: true, stepping: false, announce: "" });
  });

  it("from the start shows nothing, and stepping forward moves one step at a time", () => {
    expect(reached(steps, AT_START)).toBe(0);
    expect(stepBar(steps, AT_START)).toMatchObject({ position: "0 / 20", atStart: true, atEnd: false, stepping: true });
    const one = nextKey(steps, AT_START);
    expect(one).toBe("turn:chat-0");
    expect(stepBar(steps, one)).toMatchObject({ position: "1 / 20", stepping: true, announce: "Event 1 of 20" });
    expect(prevKey(steps, one)).toBe(AT_START);
  });

  it("stepping onto the last step is back at the latest", () => {
    expect(nextKey(steps, steps[18].key)).toBeNull();
    expect(prevKey(steps, null)).toBe(steps[18].key);
  });

  it("Play from the latest starts over; from a step it carries on", () => {
    expect(playFrom(steps, null)).toBe(AT_START);
    expect(playFrom(steps, steps[3].key)).toBe(steps[3].key);
  });

  it("stays on the same step when steps are added before it", () => {
    const events = chatSession().events;
    const cursor = readSession(events).steps.find((s) => s.kind === "run" && s.run === 2)!.key;
    expect(stepBar(readSession(events).steps, cursor).position).toBe("10 / 20");
    // A call of run 1 recovered after a gap: appended last, placed inside run 1.
    const started = events.find((e) => e.session_seq === 7)!;
    const backfilled: StoredEvent = structuredClone(started);
    backfilled.session_seq = 44;
    backfilled.event_id = "00000000-0000-4000-8000-000000000006-backfill";
    backfilled.backfilled = true;
    const ev = (backfilled.body as { event: Record<string, unknown> }).event;
    ev.call_index = 2;
    ev.line = { line: 7, column: 5 };
    delete ev.package_line;
    (backfilled as unknown as { position: Record<string, number> }).position.call_index = 2;
    const after = readSession([...events, backfilled]).steps;
    expect(after.length).toBe(21);
    expect(after.findIndex((s) => s.key === "ev:00000000-0000-4000-8000-000000000006-backfill")).toBe(5);
    expect(stepBar(after, cursor)).toMatchObject({ position: "11 / 21", stepping: true });
    expect(after[reached(after, cursor) - 1].key).toBe(cursor);
  });

  it("stays on a chat message when its turn is replaced by the complete copy", () => {
    const events = plainSession().events;
    const before = readSession(events.slice(0, 2)).steps;
    expect(before[1]).toMatchObject({ key: "turn:chat-1", text: "Checking." });
    const after = readSession(events).steps;
    expect(after[reached(after, "turn:chat-1") - 1]).toMatchObject({ key: "turn:chat-1", text: "Checking the status page." });
  });

  it("a resumed feed's events, merged in, give the same steps", () => {
    const whole = chatSession().events;
    const resumed = feed("feed-resume-session-4.sse");
    const merged = [...whole.slice(0, 5), ...resumed];
    expect(readSession(merged).steps.map((s) => s.key)).toEqual(readSession(whole).steps.map((s) => s.key));
  });
});
