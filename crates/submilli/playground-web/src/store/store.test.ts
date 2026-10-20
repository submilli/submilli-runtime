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
  sessionStorage.clear();
  history.replaceState(null, "", "/");
});

function start(server: FakeServer, hash: string, frame?: (cb: () => void) => void, livePollMs = 0) {
  history.replaceState(null, "", "/" + hash);
  store = createStore({
    fetch: server.fetch,
    frame: frame ?? ((cb) => setTimeout(cb, 0)),
    pollMs: 5,
    livePollMs,
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

test("a session's events show with its record, then once per frame, by session_seq", async () => {
  const server = new FakeServer();
  signedIn(server);
  const frames: (() => void)[] = [];
  const { store, started } = start(server, "", (cb) => frames.push(cb));
  await started;
  // What arrived before the session was ready shows with it, not a frame later.
  await expect.poll(() => store.getSnapshot().sessionLoading).toBe(false);
  const shownWithRecord = store.getSnapshot().data.events.length;
  expect(shownWithRecord).toBeGreaterThan(0);
  // The rest waits for a frame each time.
  await expect
    .poll(() => {
      for (const run of frames.splice(0)) run();
      return store.getSnapshot().data.events.length;
    })
    .toBe(43);
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
  const count = (path: string) => server.requests().filter((r) => r.split("?")[0] === `GET ${path}`).length;
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
    if (call.url.split("?")[0] !== "/api/sessions") return undefined;
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
    if (call.url.split("?")[0] === "/api/sessions") {
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
      variables: { customerId: "pied_piper" },
    });
    expect(store.getSnapshot().pendingChat).toEqual({ message: "Hello", session: null });
    await expect.poll(() => store.getSnapshot().data.sessionId).toBe(SESSION(8));
    expect(added).toContain("beforeunload");
    const agent = server.calls.find((c) => c.url === "/api/agent");
    expect(agent?.body).toEqual({
      task: "Hello",
      model: "anthropic:claude-sonnet-5-5",
      variables: { customerId: "pied_piper" },
    });
    expect(server.requests()).toContain(`GET /api/sessions/${SESSION(8)}/events`);
    expect(store.getSnapshot().newSession).toBeNull();

    // The answer names the session the page already found.
    answer(new Response(fixture("agent-new.json").replaceAll(SESSION(4), SESSION(8))));
    const { outcome, session } = await sent;
    expect(outcome.ok).toBe(true);
    expect(session).toBe(SESSION(8));
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
  const { outcome, session } = await store.chat("And the largest?");
  expect(outcome.ok).toBe(true);
  expect(session).toBe(SESSION(4));
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
});

test("an accepted test is sent with its mode and loads the run it made", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  const outcome = await store.test(2, "reads-live");
  expect(outcome.ok).toBe(true);
  expect(server.calls.find((c) => c.url === "/api/test")?.body).toEqual({ run: 2, mode: "reads-live" });
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

// ---- what changed elsewhere, and links

const listed = (server: FakeServer) => server.sessions as SessionList;

test("a test of a shown run made elsewhere is read under it within a poll", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { started } = start(server, "", undefined, 10);
  await started;
  const reads = () => server.requests().filter((r) => r === "GET /api/runs/1?program=true").length;
  const before = reads();
  const shown = listed(server).sessions.find((s) => s.session === SESSION(4))!;
  shown.follow_ups += 1;
  await expect.poll(reads).toBeGreaterThan(before);
});

test("a session started elsewhere is noticed while a quiet session is shown", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "", undefined, 10);
  await started;
  server.sessions = sessionsWith(chatSession(8));
  await expect.poll(() => store.getSnapshot().newSession).toBe(SESSION(8));
  expect(store.getSnapshot().data.sessionId).toBe(SESSION(4));
});

test("with no sessions yet, the first one made elsewhere opens", async () => {
  const server = new FakeServer();
  server.sessions = { sessions: [], more: 0 };
  signedIn(server);
  const { store, started } = start(server, "", undefined, 10);
  await started;
  expect(store.getSnapshot().noSessions).toBe(true);
  server.sessions = fixtureJson("sessions.json");
  await expect.poll(() => store.getSnapshot().data.sessionId).toBe(SESSION(4));
});

test("a run's link pasted into the open page shows that run's session", async () => {
  const server = new FakeServer();
  signedIn(server);
  server.sessions = sessionsWith(chatSession(8));
  const { store, started } = start(server, "");
  await started;
  expect(store.getSnapshot().data.sessionId).toBe(SESSION(8));
  location.hash = "#run=2";
  window.dispatchEvent(new HashChangeEvent("hashchange"));
  await expect.poll(() => store.getSnapshot().data.sessionId).toBe(SESSION(4));
  expect(store.getSnapshot().requested).toEqual({ run: 2, decision: null });
});

test("a login link opened again while signed in keeps the page signed in", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "#login=used-already");
  await started;
  expect(store.getSnapshot().auth).toBe("signed-in");
});

test("a reply waits in the session it was sent to", async () => {
  const server = new FakeServer();
  signedIn(server);
  const { store, started } = start(server, "");
  await started;
  const sent = store.chat("And the largest?");
  expect(store.getSnapshot().pendingChat).toEqual({ message: "And the largest?", session: SESSION(4) });
  await sent;
});

test("a feed the server answers 503 is tried again, not ended", async () => {
  const server = new FakeServer();
  signedIn(server);
  let refused = 0;
  server.handle = (call) => (call.url.endsWith("/events") && refused++ === 0 ? new Response("busy", { status: 503 }) : undefined);
  const { store, started } = start(server, "");
  await started;
  await expect.poll(() => store.getSnapshot().data.events.length).toBe(43);
  expect(store.getSnapshot().feedError).toBeNull();
});

test("a reload comes back to the session the tab showed, not the newest", async () => {
  const server = new FakeServer();
  localStorage.setItem(SESSION_KEY, server.token);
  const first = createStore({ fetch: server.fetch, frame: (cb) => setTimeout(cb, 0), pollMs: 5, retryDelay: () => 0 });
  await first.start();
  const list = fixtureJson<SessionList>("sessions.json").sessions;
  const other = list.find((s) => s.session !== first.getSnapshot().data.sessionId)!.session;
  await first.openSession(other);
  first.dispose();
  store = createStore({ fetch: server.fetch, frame: (cb) => setTimeout(cb, 0), pollMs: 5, retryDelay: () => 0 });
  await store.start();
  expect(store.getSnapshot().data.sessionId).toBe(other);
});

test("a session opened while the last one had a flush waiting still shows its live events", async () => {
  const server = new FakeServer();
  signedIn(server);
  // Session 4's feed stays open, so one more event can arrive while the page is on it.
  let more: ReadableStreamDefaultController<Uint8Array> | null = null;
  const live = (text: string) => more?.enqueue(new TextEncoder().encode(text));
  const one = fixture("feed-session-1.sse").split("\n\n")[0] + "\n\n";
  let fed: ReadableStreamDefaultController<Uint8Array> | null = null;
  server.handle = (call) => {
    if (call.url === `/api/sessions/${SESSION(4)}/events`)
      return new Response(new ReadableStream({ start: (c) => { more = c; c.enqueue(new TextEncoder().encode(fixture("feed-session-4.sse"))); } }), { headers: { "content-type": "text/event-stream" } });
    // Session 1 sends nothing at first, then its run's start, as a run in flight does.
    if (call.url === `/api/sessions/${SESSION(1)}/events`)
      return new Response(new ReadableStream({ start: (c) => (fed = c) }), { headers: { "content-type": "text/event-stream" } });
    return undefined;
  };
  const frames: (() => void)[] = [];
  const runFrames = () => frames.splice(0).forEach((cb) => cb());
  const { store: s, started } = start(server, "", (cb) => frames.push(cb));
  await started;
  runFrames();
  // An event on session 4 waits for its frame, and session 1 opens before it comes.
  live(fixture("feed-session-4.sse").split("\n\n")[0].replace(/"session_seq":\d+/, '"session_seq":999') + "\n\n");
  await new Promise((r) => setTimeout(r, 10));
  const opened = s.openSession(SESSION(1));
  runFrames();
  await opened;
  fed!.enqueue(new TextEncoder().encode(one));
  await new Promise((r) => setTimeout(r, 10));
  runFrames();
  expect(s.getSnapshot().data.events.map((e) => e.session_seq)).toEqual([1]);
});

test("a run still going in the shown session has its program read from the run list, once", async () => {
  const server = new FakeServer();
  signedIn(server);
  const started9 = {
    format: 1,
    session_seq: 999,
    event_id: "started-9",
    position: { at_micros: 1791898099000000, run: 9, rank: 0 },
    run: 9,
    body: { event: { schema: 1, event_id: "started-9", seq: 999, at_micros: 1791898099000000, session_id: SESSION(4), kind: "run-started", label: "chat", entry: "mcp" } },
  };
  const program = "function main(): number {\n  return 1;\n}";
  const listing = `/api/runs?session=${encodeURIComponent(SESSION(4))}&limit=1`;
  server.handle = (call) => {
    if (call.url === `/api/sessions/${SESSION(4)}/events`)
      return new Response(fixture("feed-session-4.sse").trimEnd() + "\n\nid: 999\nevent: event\ndata: " + JSON.stringify(started9) + "\n\n", { headers: { "content-type": "text/event-stream" } });
    if (call.url === listing)
      return Response.json({ runs: [], running: [{ run: 9, source: "chat", started_at_micros: 1, session: SESSION(4), untrusted: { program } }] });
    return undefined;
  };
  const { store, started } = start(server, "", (cb) => setTimeout(cb, 0));
  await started;
  await expect.poll(() => store.getSnapshot().data.running[9]).toBe(program);
  // The runs that finished were never asked about, and run 9 only once.
  expect(server.requests().filter((r) => r === "GET " + listing)).toHaveLength(1);
});

test("a session the list leaves out, as an old rerun's own, is asked for by name when opened", async () => {
  const server = new FakeServer();
  signedIn(server);
  const all = fixtureJson<SessionList>("sessions.json");
  const rerun = all.sessions.find((s) => s.sources.includes("rerun"))!;
  server.sessions = { ...all, sessions: all.sessions.filter((s) => s !== rerun) };
  server.handle = (call) => (call.url === "/api/sessions?session=" + encodeURIComponent(rerun.session) ? Response.json(all) : undefined);
  const { store, started } = start(server, "");
  await started;
  expect(store.getSnapshot().data.runs[7]).toBeUndefined();

  await store.openSession(rerun.session);
  expect(store.getSnapshot().data.sessions.some((s) => s.session === rerun.session)).toBe(true);
  expect(store.getSnapshot().data.runs[7]?.run).toBe(7);
  // Read again with the list, it stays listed while shown.
  await store.refreshSessions();
  expect(store.getSnapshot().data.sessions.some((s) => s.session === rerun.session)).toBe(true);
});

test("a reload comes back to the session it showed, though the list leaves it out", async () => {
  const server = new FakeServer();
  signedIn(server);
  const all = fixtureJson<SessionList>("sessions.json");
  const rerun = all.sessions.find((s) => s.sources.includes("rerun"))!;
  server.sessions = { ...all, sessions: all.sessions.filter((s) => s !== rerun) };
  server.handle = (call) => (call.url === "/api/sessions?session=" + encodeURIComponent(rerun.session) ? Response.json(all) : undefined);
  sessionStorage.setItem("submilli-playground-shown-session", rerun.session);
  const { store, started } = start(server, "");
  await started;
  expect(store.getSnapshot().data.sessionId).toBe(rerun.session);
});
