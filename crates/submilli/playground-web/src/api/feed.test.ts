import { afterEach, expect, test } from "vitest";

import type { FeedSignal, StoredEvent } from "./types";
import { Client, SESSION_KEY } from "./client";
import { createSseParser, openFeed, type FeedState, type SseMessage } from "./feed";
import { FakeServer, SESSION, fixture } from "./testing/fakeServer";

afterEach(() => localStorage.clear());

function follow(server: FakeServer, session: string) {
  localStorage.setItem(SESSION_KEY, server.token);
  const client = new Client({ fetch: server.fetch });
  const events: StoredEvent[] = [];
  const signals: FeedSignal[] = [];
  const states: FeedState[] = [];
  const feed = openFeed({
    client,
    session,
    onEvent: (e) => events.push(e),
    onSignal: (s) => signals.push(s),
    onState: (s) => states.push(s),
    retryDelay: () => 0,
  });
  return { events, signals, states, feed };
}

test("the parser joins data lines, skips comments, and dispatches on a blank line", () => {
  const got: SseMessage[] = [];
  const parser = createSseParser((m) => got.push(m));
  parser.push("id: 7\nevent: chat\ndata: {\"a\":\ndata: 1}\n\n: keep-alive\n\nevent: run-");
  parser.push("idle\r\ndata: {}\r\n\r\n");
  expect(got).toEqual([
    { id: "7", event: "chat", data: '{"a":\n1}' },
    { id: null, event: "run-idle", data: "{}" },
  ]);
});

test("the reader parses every kind the recorded session sends, across split chunks", async () => {
  const server = new FakeServer();
  const { events, signals, feed } = follow(server, SESSION(4));
  await expect.poll(() => signals.length).toBe(1);
  feed.stop();
  expect(events.map((e) => e.session_seq)).toEqual(Array.from({ length: 43 }, (_, i) => i + 1));
  const kinds = new Set(events.map((e) => Object.keys(e.body)[0]));
  expect(kinds).toEqual(new Set(["chat", "chat-tools", "event"]));
  expect(signals).toEqual([{ kind: "run-idle", session: SESSION(4) }]);
});

test("a gap, a replaced chat and a feed error are parsed, and the error ends the feed", async () => {
  const server = new FakeServer();
  server.feed = () => ({ text: fixture("data-feed-gap-error.sse"), close: true, chunk: 50 });
  const { events, signals, states } = follow(server, SESSION(4));
  await expect.poll(() => states.at(-1)).toBe("ended");
  expect(events.map((e) => Object.keys(e.body)[0])).toEqual(["chat", "gap", "chat-replaced"]);
  expect(events[1].body).toEqual({
    gap: { run_id: "00000000-0000-4000-8000-000000000007", dropped: 3, recovered: 2, lost: false },
  });
  expect(signals).toEqual([
    {
      kind: "feed-error",
      session: SESSION(4),
      message: "the session's event log could not be read",
    },
  ]);
  // Closed after the error, and not opened again.
  await new Promise((r) => setTimeout(r, 20));
  expect(server.requests().filter((r) => r.includes("/events"))).toHaveLength(1);
});

test("session-ended ends the feed without reconnecting", async () => {
  const server = new FakeServer();
  server.feed = () => ({ text: fixture("feed-session-1.sse"), close: true });
  const { events, signals, states } = follow(server, SESSION(1));
  await expect.poll(() => states.at(-1)).toBe("ended");
  expect(events).toHaveLength(9);
  expect(signals.map((s) => s.kind)).toEqual(["run-idle", "session-ended"]);
  await new Promise((r) => setTimeout(r, 20));
  expect(server.requests().filter((r) => r.includes("/events"))).toHaveLength(1);
});

test("after a dropped connection it resumes with Last-Event-ID and duplicates nothing", async () => {
  const server = new FakeServer();
  const full = fixture("feed-session-4.sse");
  // The first connection drops partway into the sixth event.
  const cut = full.indexOf("id: 6\n") + 40;
  server.feed = (_session, lastEventId) =>
    lastEventId === null
      ? { text: full.slice(0, cut), close: true }
      : { text: fixture("feed-resume-session-4.sse"), close: false };
  const { events, states, feed } = follow(server, SESSION(4));
  await expect.poll(() => events.length).toBe(43);
  feed.stop();
  const feeds = server.calls.filter((c) => c.url.endsWith("/events"));
  expect(feeds.map((c) => c.headers.get("last-event-id"))).toEqual([null, "5"]);
  expect(events.map((e) => e.session_seq)).toEqual(Array.from({ length: 43 }, (_, i) => i + 1));
  expect(states).toContain("reconnecting");
});

test("the feed's URL never carries the token", async () => {
  const server = new FakeServer();
  const { signals, feed } = follow(server, SESSION(4));
  await expect.poll(() => signals.length).toBe(1);
  feed.stop();
  const call = server.calls.find((c) => c.url.endsWith("/events"));
  expect(call?.url).toBe(`/api/sessions/${SESSION(4)}/events`);
  expect(call?.headers.get("authorization")).toBe(`Bearer ${server.token}`);
});
