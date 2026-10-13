import { afterEach, expect, test } from "vitest";

import type { SessionList } from "../api/types";
import { SESSION_KEY } from "../api/client";
import { FakeServer, SESSION, fixture, fixtureJson } from "../api/testing/fakeServer";
import { createStore, type PageStore } from "./store";

let store: PageStore | null = null;

afterEach(() => {
  store?.dispose();
  store = null;
  localStorage.clear();
  history.replaceState(null, "", "/");
});

function start(server: FakeServer, hash: string, frame?: (cb: () => void) => void) {
  history.replaceState(null, "", "/" + hash);
  store = createStore({
    fetch: server.fetch,
    frame: frame ?? ((cb) => setTimeout(cb, 0)),
    pollMs: 5,
    retryDelay: () => 0,
  });
  const started = store.start();
  return { store, started };
}

function signedIn(server: FakeServer) {
  localStorage.setItem(SESSION_KEY, server.token);
}

const sessionsWith = (...extra: object[]): SessionList => {
  const list = fixtureJson<SessionList>("sessions.json");
  return { ...list, sessions: [...(extra as SessionList["sessions"]), ...list.sessions] };
};

const chatSession = (n: number, sources = ["chat"]) => ({
  session: SESSION(n),
  started_at: "2026-10-13T13:30:00Z",
  started_at_micros: 1791898100000000 + n,
  sources,
  variables: {},
  run_count: 0,
  runs: [],
  denied: false,
  open: true,
});

test("a link's login code is exchanged once, leaves the address bar, and its run is opened", async () => {
  const server = new FakeServer();
  const { store, started } = start(server, "#login=good&run=2&decision=1");
  await started;
  expect(server.calls.filter((c) => c.url === "/api/login")).toHaveLength(1);
  expect(server.calls[0].body).toEqual({ code: "good" });
  expect(localStorage.getItem(SESSION_KEY)).toBe(server.token);
  expect(location.hash).toBe("#run=2&decision=1");
  const snap = store.getSnapshot();
  expect(snap.auth).toBe("signed-in");
  expect(snap.requested).toEqual({ run: 2, decision: 1 });
  expect(snap.data.sessionId).toBe(SESSION(4));
  // The session's runs, the follow-ups listed under them, and how each follow-up compares.
  await expect.poll(() => Object.keys(store.getSnapshot().data.runs).sort()).toEqual(
    ["1", "2", "3", "4", "5", "6", "7"],
  );
  await expect.poll(() => Object.keys(store.getSnapshot().data.compares).sort()).toEqual([
    "5",
    "6",
    "7",
  ]);
  expect(store.getSnapshot().data.runs[2].untrusted.program).toContain("listCharges");
  await expect.poll(() => store.getSnapshot().data.events.length).toBe(43);
});

test("an expired login code shows the signed-out screen and sends no other request", async () => {
  const server = new FakeServer();
  const { store, started } = start(server, "#login=expired");
  await started;
  expect(store.getSnapshot().auth).toBe("signed-out");
  expect(server.requests()).toEqual(["POST /api/login"]);
  expect(location.hash).toBe("");
});

test("a 401 signs the page out, keeps the requested run, and opens it after the next login", async () => {
  const server = new FakeServer();
  const { store, started } = start(server, "#login=good&run=5");
  await started;
  // Run 5 is a test, so the session shown is the one of the run it tested.
  expect(store.getSnapshot().data.sessionId).toBe(SESSION(4));

  server.token = "rotated";
  await store.loadExplain("2.1");
  let snap = store.getSnapshot();
  expect(snap.auth).toBe("signed-out");
  expect(localStorage.getItem(SESSION_KEY)).toBeNull();
  expect(snap.requested).toEqual({ run: 5, decision: null });

  history.replaceState(null, "", "/#login=good");
  window.dispatchEvent(new HashChangeEvent("hashchange"));
  await expect.poll(() => store.getSnapshot().auth).toBe("signed-in");
  snap = store.getSnapshot();
  expect(snap.requested).toEqual({ run: 5, decision: null });
  expect(snap.data.sessionId).toBe(SESSION(4));
  expect(location.hash).toBe("#run=5");
});

test("with no run asked for, the newest session that isn't a test or rerun is opened", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  expect(store.getSnapshot().data.sessionId).toBe(SESSION(4));
  expect(store.getSnapshot().loading).toBe(false);
});

test("with no sessions at all, the store says so and opens no feed", async () => {
  const server = new FakeServer();
  server.sessions = { sessions: [], more: 0 };
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  expect(store.getSnapshot().noSessions).toBe(true);
  expect(store.getSnapshot().data.sessionId).toBeNull();
  expect(server.requests().some((r) => r.includes("/events"))).toBe(false);
});

test("events are appended once per frame, by session_seq", async () => {
  const server = new FakeServer();
  signedIn(server);
  const frames: (() => void)[] = [];
  const { store, started } = start(server, "", (cb) => frames.push(cb));
  await started;
  await expect.poll(() => frames.length).toBe(1);
  // The feed has sent everything, but nothing is shown until the frame.
  await new Promise((r) => setTimeout(r, 20));
  expect(frames).toHaveLength(1);
  expect(store.getSnapshot().data.events).toHaveLength(0);
  frames.shift()?.();
  const seqs = store.getSnapshot().data.events.map((e) => e.session_seq);
  expect(seqs).toEqual(Array.from({ length: 43 }, (_, i) => i + 1));
});

test("a gap is kept, and a feed error ends the feed with its message", async () => {
  const server = new FakeServer();
  server.feed = () => ({ text: fixture("data-feed-gap-error.sse"), close: true, chunk: 64 });
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  await expect.poll(() => store.getSnapshot().data.feedEnded).toBe(true);
  const snap = store.getSnapshot();
  expect(snap.feedError).toBe("the session's event log could not be read");
  expect(snap.data.events.map((e) => Object.keys(e.body)[0])).toEqual([
    "chat",
    "gap",
    "chat-replaced",
  ]);
});

test("status is read again on run-idle, on a finished run, and when the window regains focus", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  await expect.poll(() => store.getSnapshot().data.events.length).toBe(43);
  const count = (path: string) => server.requests().filter((r) => r === `GET ${path}`).length;
  // One at sign-in, then more for the finished runs and the run-idle; reads asked for
  // while one is under way become one more read after it.
  await expect.poll(() => count("/api/status")).toBeGreaterThan(1);
  expect(count("/api/sessions")).toBe(2);
  // Let the reads under way finish before counting.
  await new Promise((resolve) => setTimeout(resolve, 50));
  const before = count("/api/status");
  expect(before).toBeLessThan(6);
  window.dispatchEvent(new Event("focus"));
  await expect.poll(() => count("/api/status")).toBe(before + 1);
});

test("a different session starting shows a notice and doesn't switch", async () => {
  const server = new FakeServer();
  signedIn(server);
  let reads = 0;
  server.handle = (call) => {
    if (call.url !== "/api/sessions") return undefined;
    reads += 1;
    // A throwaway test session doesn't count; another chat does.
    return Response.json(
      reads === 1 ? sessionsWith() : sessionsWith(chatSession(8), chatSession(9, ["test"])),
    );
  };
  const { store, started } = start(server, "");
  await started;
  await expect.poll(() => store.getSnapshot().newSession).toBe(SESSION(8));
  expect(store.getSnapshot().data.sessionId).toBe(SESSION(4));
  store.dismissNotice();
  expect(store.getSnapshot().newSession).toBeNull();
});

test("a page-started chat is followed before its request returns, with no notice", async () => {
  const server = new FakeServer();
  signedIn(server);
  let agentSent = false;
  let answer: (r: Response) => void = () => {};
  server.handle = (call) => {
    if (call.url === "/api/sessions") {
      return Response.json(agentSent ? sessionsWith(chatSession(8, ["browser-chat"])) : sessionsWith());
    }
    if (call.url === "/api/agent") {
      agentSent = true;
      return new Promise<Response>((resolve) => (answer = resolve));
    }
    return undefined;
  };
  const added: string[] = [];
  const removed: string[] = [];
  const add = window.addEventListener.bind(window);
  const remove = window.removeEventListener.bind(window);
  window.addEventListener = ((type: string, ...rest: [EventListener]) => {
    added.push(type);
    add(type, ...rest);
  }) as typeof window.addEventListener;
  window.removeEventListener = ((type: string, ...rest: [EventListener]) => {
    removed.push(type);
    remove(type, ...rest);
  }) as typeof window.removeEventListener;
  try {
    const { store, started } = start(server, "");
    await started;
    const sent = store.chat("Hello", {
      model: "anthropic:claude-sonnet-5-5",
      variables: { customerId: "cus_northwind" },
    });
    expect(store.getSnapshot().pendingChat).toEqual({ message: "Hello", newSession: true });
    await expect.poll(() => store.getSnapshot().data.sessionId).toBe(SESSION(8));
    expect(added).toContain("beforeunload");
    const agent = server.calls.find((c) => c.url === "/api/agent");
    expect(agent?.body).toEqual({
      task: "Hello",
      model: "anthropic:claude-sonnet-5-5",
      variables: { customerId: "cus_northwind" },
    });
    expect(server.requests()).toContain(`GET /api/sessions/${SESSION(8)}/events`);
    expect(store.getSnapshot().newSession).toBeNull();

    // The answer names the session the page already found.
    answer(new Response(fixture("agent-new.json").replaceAll(SESSION(4), SESSION(8))));
    const outcome = await sent;
    expect(outcome.ok).toBe(true);
    const snap = store.getSnapshot();
    expect(snap.pendingChat).toBeNull();
    expect(removed).toContain("beforeunload");
    expect(snap.data.sessionId).toBe(SESSION(8));
    const polls = server.requests().filter((r) => r === "GET /api/sessions").length;
    await new Promise((r) => setTimeout(r, 30));
    expect(server.requests().filter((r) => r === "GET /api/sessions").length).toBe(polls);
  } finally {
    window.addEventListener = add;
    window.removeEventListener = remove;
  }
});

test("a reply goes to the shown session", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  const outcome = await store.chat("And the largest?");
  expect(outcome.ok).toBe(true);
  expect(server.calls.find((c) => c.url === "/api/agent")?.body).toEqual({
    task: "And the largest?",
    session: SESSION(4),
  });
});

test("a refused cancel is shown as a refusal with its message", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  const outcome = await store.cancel(1);
  expect(outcome).toEqual({
    ok: false,
    kind: "not-in-flight",
    message: "run 1 already finished; there is nothing to cancel",
  });
  expect(store.getSnapshot().action).toEqual({
    name: "cancel",
    run: 1,
    state: "refused",
    message: "run 1 already finished; there is nothing to cancel",
  });
});

test("an accepted test is sent with its mode and loads the run it made", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  const outcome = await store.test(2, "reads-live");
  expect(outcome.ok).toBe(true);
  expect(server.calls.find((c) => c.url === "/api/test")?.body).toEqual({ run: 2, mode: "reads-live" });
  expect(store.getSnapshot().action).toMatchObject({ name: "test", run: 2, state: "accepted" });
  await expect.poll(() => store.getSnapshot().data.runs[6]?.run).toBe(6);
});

test("an accepted test or rerun reads the status again, since its run ends in another session", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  await expect.poll(() => store.getSnapshot().data.events.length).toBe(43);
  await new Promise((resolve) => setTimeout(resolve, 50));
  const reads = () => server.requests().filter((r) => r === "GET /api/status").length;
  const before = reads();
  await store.test(2);
  await expect.poll(reads).toBe(before + 1);
  await store.rerun(1);
  await expect.poll(reads).toBe(before + 2);
});

test("a drafted rule is returned for the page to show", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  const outcome = await store.draft("2.1");
  expect(server.calls.find((c) => c.url === "/api/draft-rule")?.body).toEqual({ decision: "2.1" });
  expect(outcome.ok && outcome.result.lines).toEqual([39, 41]);
});

test("explains, versions and sources load once, and a missing source says why", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  await Promise.all([store.loadExplain("2.1"), store.loadExplain("2.1")]);
  await store.loadVersion(1);
  await store.loadVersion(1);
  await store.loadSource("@acme/billing", "lib");
  await store.loadSource("@acme/billing", "nope");
  const once = (prefix: string) => server.requests().filter((r) => r.startsWith(prefix)).length;
  expect(once("GET /api/decisions/2.1")).toBe(1);
  expect(once("GET /api/changes?version=1")).toBe(1);
  const snap = store.getSnapshot();
  expect(snap.data.explains["2.1"].outcome).toBe("deny");
  expect(snap.data.versions[1].text).toContain("kind: blueprint");
  expect(snap.data.sources["@acme/billing\u0000lib"].untrusted.text).toContain("listCharges");
  expect(snap.sourceErrors["@acme/billing\u0000nope"]).toEqual({
    code: "unknown_module",
    message: "package `@acme/billing` has no module `nope`",
  });
  expect(server.requests()).toContain("GET /api/packages/source?package=%40acme%2Fbilling&module=lib");
});

test("opening another session stops the current feed and loads the other", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  await expect.poll(() => store.getSnapshot().data.events.length).toBe(43);
  const first = server.calls.find((c) => c.url.endsWith("/events"));
  await store.openSession(SESSION(1));
  await expect.poll(() => store.getSnapshot().data.feedEnded).toBe(true);
  const snap = store.getSnapshot();
  expect(snap.data.sessionId).toBe(SESSION(1));
  expect(snap.data.events).toHaveLength(9);
  expect(first).toBeDefined();
  expect(server.requests()).toContain(`GET /api/sessions/${SESSION(1)}/events`);
});

test("no request URL ever carries the token", async () => {
  const server = new FakeServer();
  const { store, started } = start(server, "#login=good&run=2");
  await started;
  await expect.poll(() => store.getSnapshot().data.events.length).toBe(43);
  await store.loadSource("@acme/billing", "lib");
  await store.test(2);
  await store.chat("again");
  expect(server.calls.length).toBeGreaterThan(10);
  for (const call of server.calls) expect(call.url).not.toContain(server.token);
});
