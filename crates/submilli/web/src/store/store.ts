// Everything the page has loaded, kept current from the session feed. Components read
// it with `useStore` and call the store's methods; nothing else talks to the server.

import type {
  AgentResult,
  BlueprintVersion,
  Changes,
  Compare,
  Draft,
  Explain,
  FeedSignal,
  PackageSource,
  PageData,
  SessionList,
  SessionSummary,
  Show,
  Status,
  StoredEvent,
} from "../api/types";
import { sourceKey } from "../api/types";
import { Client, ReadError, SESSION_KEY, SignedOutError, type ActionOutcome } from "../api/client";
import { openFeed, type FeedState } from "../api/feed";
import { SIGNED_OUT_LINE, exchangeCode, readLink, runHash, type RequestedRun } from "../api/login";

export type TestMode = "recorded" | "reads-live" | "live";

/** What a new chat session starts with; a reply carries none of it. */
export interface NewSessionSettings {
  /** As `<provider>:<model>`, one of the status's `chat_models`. */
  model: string;
  effort?: string;
  max_output?: number;
  provider_settings?: Record<string, unknown>;
  variables: Record<string, string>;
}

/** The last action the page asked for, and how it went. */
export interface ActionState {
  name: "test" | "rerun" | "cancel" | "draft" | "chat";
  run?: number;
  decision?: string;
  /** `failed` when the playground didn't answer at all. */
  state: "pending" | "accepted" | "refused" | "failed";
  /** The refusal, in the CLI's words. */
  message: string | null;
}

export interface Snapshot {
  data: PageData;
  /** `checking` until the token is known to work or not. */
  auth: "checking" | "signed-in" | "signed-out";
  /** The run the link asked for; kept across a sign-out so it opens after the next login. */
  requested: RequestedRun | null;
  /** The first load after sign-in is still on its way. */
  loading: boolean;
  /** Why the first load failed, if it did. */
  loadError: string | null;
  /** Signed in, and the playground has recorded no session yet. */
  noSessions: boolean;
  /** `idle` with no session open; `reconnecting` while a dropped feed is opened again. */
  feed: FeedState | "idle";
  /** The feed's own error, which ended it. */
  feedError: string | null;
  /** A different session that started while this one was followed live. */
  newSession: string | null;
  /** A chat the page sent and is still waiting on. */
  pendingChat: { message: string; newSession: boolean } | null;
  action: ActionState | null;
  /** Package sources that couldn't be shown, by `sourceKey`, with the server's reason. */
  sourceErrors: Record<string, { code: string; message: string }>;
}

export interface StoreOptions {
  fetch?: typeof fetch;
  /** Runs a batch of feed events into the page; one call per animation frame. */
  frame?: (callback: () => void) => void;
  /** How often to look for a page-started chat's session, in milliseconds. */
  pollMs?: number;
  retryDelay?: (attempt: number) => number;
}

export type PageStore = ReturnType<typeof createStore>;

const THROWAWAY = new Set(["test", "rerun"]);

/** A session that only holds a test or a rerun; the page shows those under their run. */
export function isThrowaway(session: SessionSummary): boolean {
  return session.sources.length > 0 && session.sources.every((s) => THROWAWAY.has(s));
}

const emptyData = (): PageData => ({
  status: null,
  sessions: [],
  sessionId: null,
  events: [],
  feedEnded: false,
  runs: {},
  explains: {},
  compares: {},
  versions: {},
  sources: {},
});

export function createStore(options: StoreOptions = {}) {
  const frame =
    options.frame ??
    ((callback: () => void) =>
      typeof requestAnimationFrame === "function"
        ? requestAnimationFrame(() => callback())
        : setTimeout(callback, 16));
  const pollMs = options.pollMs ?? 500;

  let snap: Snapshot = {
    data: emptyData(),
    auth: "checking",
    requested: null,
    loading: false,
    loadError: null,
    noSessions: false,
    feed: "idle",
    feedError: null,
    newSession: null,
    pendingChat: null,
    action: null,
    sourceErrors: {},
  };
  const listeners = new Set<() => void>();

  const set = (patch: Partial<Snapshot>) => {
    snap = { ...snap, ...patch };
    for (const listener of listeners) listener();
  };
  const setData = (patch: Partial<PageData>) => set({ data: { ...snap.data, ...patch } });

  const client = new Client({ fetch: options.fetch, onSignedOut: () => signedOut() });

  let feed: { stop(): void } | null = null;
  // Bumped each time a session is opened, so a stopped feed's late events are dropped.
  let generation = 0;
  let queue: StoredEvent[] = [];
  let frameQueued = false;
  // Every session id seen, so a new one can be told apart.
  let known = new Set<string>();
  // The session a page-started chat made; it never raises the "new session" notice.
  let ownChat: string | null = null;
  // While a new chat is pending: the sessions there were before it was sent.
  let awaitingChat: Set<string> | null = null;
  const runLoads = new Map<number, Promise<void>>();
  const loads = new Map<string, Promise<void>>();

  // ---- signing in ----

  const signIn = async (hash: string) => {
    const link = readLink(hash);
    const requested = link.requested ?? snap.requested;
    set({ auth: "checking", requested });
    if (link.code) {
      // The code works once; keep it out of history, but keep the run it asked for.
      history.replaceState(null, "", location.pathname + location.search + runHash(requested));
      if (!(await exchangeCode(link.code, options.fetch))) {
        set({ auth: "signed-out" });
        return;
      }
    }
    if (!localStorage.getItem(SESSION_KEY)) {
      set({ auth: "signed-out" });
      return;
    }
    await load();
  };

  const signedOut = () => {
    feed?.stop();
    feed = null;
    generation += 1;
    awaitingChat = null;
    set({ auth: "signed-out", feed: "idle", loading: false });
  };

  const load = async () => {
    set({ loading: true, loadError: null });
    let status: Status;
    let list: SessionList;
    try {
      [status, list] = await Promise.all([
        client.read<Status>("/api/status"),
        client.read<SessionList>("/api/sessions"),
      ]);
    } catch (error) {
      if (!(error instanceof SignedOutError)) {
        set({ loading: false, loadError: error instanceof Error ? error.message : String(error) });
      }
      return;
    }
    known = new Set(list.sessions.map((s) => s.session));
    set({ auth: "signed-in" });
    setData({ status, sessions: list.sessions });
    const id =
      (snap.requested && (await sessionOfRun(snap.requested.run))) ??
      list.sessions.find((s) => !isThrowaway(s))?.session ??
      null;
    if (id === null) {
      set({ loading: false, noSessions: true });
      return;
    }
    openSession(id);
    set({ loading: false });
  };

  /** The session a run is shown in: a test or rerun is shown under the run it followed. */
  const sessionOfRun = async (run: number): Promise<string | null> => {
    await loadRun(run);
    const show = snap.data.runs[run];
    if (!show) return null;
    const source = show.test?.source_run ?? show.rerun_of ?? null;
    if (source === null) return show.session;
    await loadRun(source);
    return snap.data.runs[source]?.session ?? show.session;
  };

  // ---- the session and its feed ----

  const openSession = (id: string) => {
    feed?.stop();
    generation += 1;
    const mine = generation;
    queue = [];
    set({ feedError: null, noSessions: false, newSession: null });
    setData({ sessionId: id, events: [], feedEnded: false });
    const summary = snap.data.sessions.find((s) => s.session === id);
    for (const run of summary?.runs ?? []) void loadRun(run);
    feed = openFeed({
      client,
      session: id,
      retryDelay: options.retryDelay,
      onEvent: (event) => mine === generation && received(event),
      onSignal: (signal) => mine === generation && signalled(signal),
      onState: (state) => mine === generation && set({ feed: state }),
    });
  };

  const received = (event: StoredEvent) => {
    queue.push(event);
    if (!frameQueued) {
      frameQueued = true;
      const mine = generation;
      frame(() => mine === generation && flush());
    }
    const body = event.body;
    if ("event" in body && body.event.kind === "run-finished" && event.run !== undefined) {
      void loadRun(event.run);
      void refreshStatus();
    }
  };

  /** Adds the queued events, in `session_seq` order, with none twice. */
  const flush = () => {
    frameQueued = false;
    if (queue.length === 0) return;
    const fresh = queue.sort((a, b) => a.session_seq - b.session_seq);
    queue = [];
    const events = snap.data.events;
    const last = events.at(-1)?.session_seq ?? 0;
    if (fresh[0].session_seq > last) {
      setData({ events: events.concat(dedupe(fresh)) });
    } else {
      const bySeq = new Map(events.map((e) => [e.session_seq, e]));
      for (const e of fresh) bySeq.set(e.session_seq, e);
      setData({ events: [...bySeq.values()].sort((a, b) => a.session_seq - b.session_seq) });
    }
  };

  const signalled = (signal: FeedSignal) => {
    if (signal.kind === "run-idle") {
      void refreshStatus();
      void refreshSessions();
      return;
    }
    // Whatever arrived before the end is shown with it.
    flush();
    setData({ feedEnded: true });
    if (signal.kind === "feed-error") set({ feedError: signal.message });
  };

  // Opening a session replays every `run-finished`, so reads asked for while one is
  // under way become one more read after it, not one each.
  let statusReading = false;
  let statusAgain = false;
  const refreshStatus = async () => {
    if (statusReading) {
      statusAgain = true;
      return;
    }
    statusReading = true;
    try {
      do {
        statusAgain = false;
        try {
          setData({ status: await client.read<Status>("/api/status") });
        } catch {
          // The status shown stays as it was; a sign-out is handled by the client.
        }
      } while (statusAgain);
    } finally {
      statusReading = false;
    }
  };

  const refreshSessions = async () => {
    let list: SessionList;
    try {
      list = await client.read<SessionList>("/api/sessions");
    } catch {
      return;
    }
    setData({ sessions: list.sessions });
    if (adoptOwnChat(list)) return;
    const following = snap.data.sessionId !== null && !snap.data.feedEnded;
    for (const s of list.sessions) {
      if (known.has(s.session)) continue;
      known.add(s.session);
      if (following && !isThrowaway(s) && s.session !== ownChat && s.session !== snap.data.sessionId) {
        set({ newSession: s.session });
      }
    }
  };

  /** Opens the session a pending page chat made, once it is listed. */
  const adoptOwnChat = (list: SessionList): boolean => {
    const before = awaitingChat;
    if (!before) return false;
    const made = list.sessions.find(
      (s) => !before.has(s.session) && s.sources.includes("browser-chat"),
    );
    if (!made) return false;
    awaitingChat = null;
    ownChat = made.session;
    known.add(made.session);
    setData({ sessions: list.sessions });
    openSession(made.session);
    return true;
  };

  // ---- loads ----

  /** A run's record with its program; then its follow-ups and how they compare to it. */
  const loadRun = (run: number, again = false): Promise<void> => {
    if (!again && snap.data.runs[run]) return Promise.resolve();
    const pending = runLoads.get(run);
    if (pending && !again) return pending;
    const loading = (async () => {
      try {
        const show = await client.read<Show>(`/api/runs/${run}?program=true`);
        setData({ runs: { ...snap.data.runs, [run]: show } });
        for (const followUp of show.reruns) {
          void loadRun(followUp.run);
          void once(`compare:${followUp.run}`, async () => {
            const compare = await client.read<Compare>(`/api/compare/${run}/${followUp.run}`);
            setData({ compares: { ...snap.data.compares, [followUp.run]: compare } });
          });
        }
      } catch {
        // Not recorded yet (still running) or gone; its `run-finished` loads it again.
      }
    })();
    runLoads.set(run, loading);
    void loading.then(() => {
      if (runLoads.get(run) === loading) runLoads.delete(run);
    });
    return loading;
  };

  /** Runs a load once; one that failed may be tried again. */
  const once = (key: string, task: () => Promise<void>): Promise<void> => {
    const existing = loads.get(key);
    if (existing) return existing;
    const loading = task().catch(() => {
      loads.delete(key);
    });
    loads.set(key, loading);
    return loading;
  };

  const loadExplain = (ref: string) =>
    once(`explain:${ref}`, async () => {
      const explain = await client.read<Explain>(`/api/decisions/${encodeURIComponent(ref)}`);
      setData({ explains: { ...snap.data.explains, [ref]: explain } });
    });

  const loadVersion = (version: number) =>
    once(`version:${version}`, async () => {
      const changes = await client.read<Changes>(`/api/changes?version=${version}`);
      const found: BlueprintVersion | undefined = changes.versions.find((v) => v.version === version);
      if (found) setData({ versions: { ...snap.data.versions, [version]: found } });
    });

  /** The list of blueprint versions, read each time, keeping any text already loaded. */
  const loadVersions = async () => {
    let changes: Changes;
    try {
      changes = await client.read<Changes>("/api/changes");
    } catch {
      return;
    }
    const versions = { ...snap.data.versions };
    for (const v of changes.versions) versions[v.version] = { ...v, ...versions[v.version] };
    setData({ versions });
  };

  const loadSource = (pkg: string, module: string) => {
    const key = sourceKey(pkg, module);
    return once(`source:${key}`, async () => {
      const query = new URLSearchParams({ package: pkg, module });
      try {
        const source = await client.read<PackageSource>(`/api/packages/source?${query}`);
        setData({ sources: { ...snap.data.sources, [key]: source } });
      } catch (error) {
        if (!(error instanceof ReadError)) throw error;
        set({ sourceErrors: { ...snap.sourceErrors, [key]: { code: error.code, message: error.message } } });
      }
    });
  };

  // ---- actions ----

  const act = async <T>(
    action: Omit<ActionState, "state" | "message">,
    path: string,
    body: unknown,
  ): Promise<ActionOutcome<T>> => {
    set({ action: { ...action, state: "pending", message: null } });
    let outcome: ActionOutcome<T>;
    try {
      outcome = await client.act<T>(path, body);
    } catch (error) {
      if (!(error instanceof SignedOutError)) throw error;
      outcome = { ok: false, kind: "signed-out", message: SIGNED_OUT_LINE };
    }
    const state = outcome.ok ? "accepted" : outcome.kind === "unreachable" ? "failed" : "refused";
    set({ action: { ...action, state, message: outcome.ok ? null : outcome.message } });
    return outcome;
  };

  /** After a test or rerun of `run`, its record lists the new follow-up; read it again. */
  const followedUp = <T,>(run: number, outcome: ActionOutcome<T>): ActionOutcome<T> => {
    if (outcome.ok) {
      void loadRun(run, true);
      // Its run ends in a throwaway session whose events this feed never sees, so the
      // blueprint version it ran against would otherwise show only on the next re-read.
      void refreshStatus();
    }
    return outcome;
  };

  const test = async (run: number, mode?: TestMode) =>
    followedUp(run, await act<Show>({ name: "test", run }, "/api/test", mode ? { run, mode } : { run }));

  const rerun = async (run: number) =>
    followedUp(run, await act<Show>({ name: "rerun", run }, "/api/rerun", { run }));

  const cancel = (run: number) => act({ name: "cancel", run }, "/api/cancel", { run });

  const draft = (decision: string) =>
    act<Draft>({ name: "draft", decision }, "/api/draft-rule", { decision });

  const warnBeforeLeaving = (event: BeforeUnloadEvent) => {
    // Closing the tab would cancel the agent mid-answer.
    event.preventDefault();
    event.returnValue = "";
  };

  /**
   * Sends a chat message: a reply to the shown session, or the first message of a new
   * session with `settings`. The answer comes only after the agent's last reply, so a
   * new session is looked for in the session list meanwhile and followed live.
   */
  const chat = async (message: string, settings?: NewSessionSettings) => {
    const shown = snap.data.sessionId;
    const starting = settings !== undefined || shown === null;
    const body: Record<string, unknown> = { task: message };
    if (settings) {
      for (const [key, value] of Object.entries(settings)) if (value !== undefined) body[key] = value;
    } else if (shown !== null) {
      body.session = shown;
    }
    set({ pendingChat: { message, newSession: starting } });
    window.addEventListener("beforeunload", warnBeforeLeaving);
    let polling = false;
    if (starting) {
      awaitingChat = new Set([...known, ...snap.data.sessions.map((s) => s.session)]);
      polling = true;
      void (async () => {
        while (polling && awaitingChat) {
          await new Promise((resolve) => setTimeout(resolve, pollMs));
          if (!polling) return;
          try {
            adoptOwnChat(await client.read<SessionList>("/api/sessions"));
          } catch {
            // Try again on the next tick.
          }
        }
      })();
    }
    try {
      const outcome = await act<AgentResult>({ name: "chat" }, "/api/agent", body);
      if (outcome.ok && starting) {
        ownChat = outcome.result.session;
        known.add(ownChat);
        if (snap.data.sessionId !== ownChat) openSession(ownChat);
      }
      return outcome;
    } finally {
      polling = false;
      awaitingChat = null;
      window.removeEventListener("beforeunload", warnBeforeLeaving);
      set({ pendingChat: null });
    }
  };

  // ---- the window ----

  const onHashChange = () => {
    if (readLink(location.hash).code) void signIn(location.hash);
  };
  // A blueprint saved in the editor shows when the developer comes back to the page.
  const onFocus = () => {
    if (snap.auth === "signed-in") void refreshStatus();
  };
  const onVisibility = () => {
    if (document.visibilityState === "visible") onFocus();
  };

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    getSnapshot: () => snap,

    /** Signs in from the address bar's link, or the stored token, and loads the page. */
    async start() {
      window.addEventListener("hashchange", onHashChange);
      window.addEventListener("focus", onFocus);
      document.addEventListener("visibilitychange", onVisibility);
      await signIn(location.hash);
    },
    dispose() {
      feed?.stop();
      feed = null;
      generation += 1;
      awaitingChat = null;
      window.removeEventListener("hashchange", onHashChange);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("beforeunload", warnBeforeLeaving);
      document.removeEventListener("visibilitychange", onVisibility);
    },

    /** Shows another session: stops the current feed and follows that one's. */
    openSession,
    dismissNotice: () => set({ newSession: null }),

    loadExplain,
    loadVersion,
    loadVersions,
    loadSource,

    test,
    rerun,
    cancel,
    draft,
    chat,
  };
}

function dedupe(events: StoredEvent[]): StoredEvent[] {
  return events.filter((e, i) => i === 0 || e.session_seq !== events[i - 1].session_seq);
}
