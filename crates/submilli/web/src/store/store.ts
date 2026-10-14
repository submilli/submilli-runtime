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
  RunList,
  RunningRow,
  SavedKey,
  SessionList,
  SessionSummary,
  Show,
  Status,
  StoredEvent,
} from "../api/types";
import { sourceKey } from "../api/types";
import { Client, ReadError, SESSION_KEY, SignedOutError, UNREACHABLE, type ActionOutcome } from "../api/client";
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
  /** A run the link asked for that the playground hasn't recorded. */
  requestedMissing: number | null;
  /** The first load after sign-in is still on its way, up to the first session's record. */
  loading: boolean;
  /** The session just opened hasn't had its events and runs read yet. */
  sessionLoading: boolean;
  /** The playground stopped answering; set again once it does. */
  unreachable: boolean;
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
  /** A chat the page sent and is still waiting on: a reply to `session`, or a new session's first message. */
  pendingChat: { message: string; session: string | null } | null;
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
  /**
   * How often to read the status and the sessions again while the page is shown, so
   * what the CLI, the editor or another tab changed shows; 0 never does.
   */
  livePollMs?: number;
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
  const livePollMs = options.livePollMs ?? 3000;

  let snap: Snapshot = {
    data: emptyData(),
    auth: "checking",
    requested: null,
    requestedMissing: null,
    loading: false,
    sessionLoading: false,
    unreachable: false,
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
  // Whether the page still waits on the new chat, so its session opens once it is listed.
  let followNewChat = false;
  // Each session's tests and reruns as last listed, so a new one shows under its run.
  let followUps = new Map<string, number>();
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
      // A code used already (a login link opened again) leaves the stored token to try.
      if (!(await exchangeCode(link.code, options.fetch)) && !localStorage.getItem(SESSION_KEY)) {
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

  let loadRetry: ReturnType<typeof setTimeout> | null = null;
  const load = async () => {
    if (loadRetry) clearTimeout(loadRetry);
    loadRetry = null;
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
        // Tried again until it answers: the playground may be starting.
        set({ loading: false, loadError: error instanceof ReadError ? error.message : UNREACHABLE });
        loadRetry = setTimeout(() => void load(), livePollMs || 3000);
      }
      return;
    }
    known = new Set(list.sessions.map((s) => s.session));
    noteFollowUps(list);
    set({ auth: "signed-in", unreachable: false });
    setData({ status, sessions: list.sessions });
    // A run's link wins; else the session this tab showed before a reload, while it's listed; else the newest.
    const before = readShown();
    const id = (snap.requested && (await requestedSession(snap.requested.run))) ?? (before && known.has(before) ? before : newest(list));
    if (id === null) {
      set({ loading: false, noSessions: true });
      return;
    }
    await openSession(id);
    set({ loading: false });
  };

  const newest = (list: SessionList) => list.sessions.find((s) => !isThrowaway(s))?.session ?? null;

  /** The session of a run a link asked for; a run that isn't recorded is said so, once. */
  const requestedSession = async (run: number): Promise<string | null> => {
    const id = await sessionOfRun(run);
    set({ requestedMissing: id === null ? run : null });
    return id;
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

  /**
   * Shows session `id` and follows its feed. Resolves once its runs are read and its feed
   * has answered, so the page doesn't show an empty session before its record arrives.
   */
  const openSession = (id: string): Promise<void> => {
    feed?.stop();
    generation += 1;
    const mine = generation;
    queue = [];
    // A flush the last session had waiting is dropped with it; the new session's events
    // get one of their own.
    frameQueued = false;
    set({ feedError: null, noSessions: false, newSession: null, sessionLoading: true });
    setData({ sessionId: id, events: [], feedEnded: false });
    keepShown(id);
    const summary = snap.data.sessions.find((s) => s.session === id);
    const runs = Promise.all((summary?.runs ?? []).map((run) => loadRun(run)));
    let answered: () => void = () => {};
    const feedAnswered = new Promise<void>((resolve) => (answered = resolve));
    feed = openFeed({
      client,
      session: id,
      retryDelay: options.retryDelay,
      onEvent: (event) => mine === generation && received(event),
      onSignal: (signal) => {
        if (mine !== generation) return;
        answered();
        signalled(signal);
      },
      onState: (state) => {
        if (mine !== generation) return;
        if (state !== "connecting") answered();
        set({ feed: state });
      },
    });
    // A feed that doesn't answer soon doesn't hold the page up; it shows as reconnecting.
    const patience = new Promise<void>((resolve) => setTimeout(resolve, 5000));
    return Promise.race([Promise.all([runs, feedAnswered]), patience]).then(() => {
      if (mine !== generation) return;
      // What the feed sent first is shown with the record, not a frame later.
      flush();
      set({ sessionLoading: false });
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
          if (snap.unreachable) set({ unreachable: false });
        } catch (error) {
          // The status shown stays as it was; a sign-out is handled by the client.
          if (!(error instanceof ReadError) && !(error instanceof SignedOutError)) set({ unreachable: true });
        }
      } while (statusAgain);
    } finally {
      statusReading = false;
    }
  };

  /** Notes each session's count of tests and reruns; returns the sessions whose count changed. */
  const noteFollowUps = (list: SessionList): string[] => {
    const changed: string[] = [];
    for (const s of list.sessions) {
      const before = followUps.get(s.session);
      if (before !== undefined && before !== s.follow_ups) changed.push(s.session);
      followUps.set(s.session, s.follow_ups);
    }
    return changed;
  };

  const refreshSessions = async () => {
    let list: SessionList;
    try {
      list = await client.read<SessionList>("/api/sessions");
    } catch {
      return;
    }
    setData({ sessions: list.sessions });
    const shown = snap.data.sessionId;
    const summary = list.sessions.find((s) => s.session === shown);
    // A test or rerun of a shown run, started anywhere, lists under that run.
    const changed = noteFollowUps(list);
    if (summary && changed.includes(summary.session)) for (const run of summary.runs) void loadRun(run, true);
    // A run of the shown session that ended before the feed told us is read too.
    for (const run of summary?.runs ?? []) void loadRun(run);
    if (adoptOwnChat(list)) return;
    if (snap.noSessions && !awaitingChat) {
      const first = newest(list);
      if (first !== null) {
        for (const s of list.sessions) known.add(s.session);
        void openSession(first);
        return;
      }
    }
    for (const s of list.sessions) {
      if (known.has(s.session)) continue;
      known.add(s.session);
      if (shown !== null && !isThrowaway(s) && s.session !== ownChat && s.session !== shown) {
        set({ newSession: s.session });
      }
    }
  };

  /** Opens the session a pending page chat made, once it is listed, if the page still waits on it. */
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
    if (followNewChat) {
      // The message waits for its answer in the session it started.
      if (snap.pendingChat) set({ pendingChat: { ...snap.pendingChat, session: made.session } });
      void openSession(made.session);
    }
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

  /** A run's record again, with its result and error whole rather than cut. */
  const loadPayloads = (run: number) =>
    once(`payloads:${run}`, async () => {
      const show = await client.read<Show>(`/api/runs/${run}?program=true&include_payloads=true`);
      setData({ runs: { ...snap.data.runs, [run]: show } });
    });

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
    signal?: AbortSignal,
  ): Promise<ActionOutcome<T>> => {
    set({ action: { ...action, state: "pending", message: null } });
    let outcome: ActionOutcome<T>;
    try {
      outcome = await client.act<T>(path, body, signal);
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
      // The new run too: read as it finished, it may not have said yet which run it followed.
      const made = (outcome.result as { run?: unknown }).run;
      if (typeof made === "number") void loadRun(made, true);
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

  /**
   * The run a test or rerun from the page started, while it's in flight: the newest
   * running run under that label that started at `since` (epoch microseconds) or later.
   * It runs in a throwaway session, so only the run list knows it. Looked for a few times,
   * as the run is numbered a moment after the request goes.
   */
  const followUpInFlight = async (label: "test" | "rerun", since: number): Promise<RunningRow | null> => {
    for (let tries = 0; tries < 5; tries++) {
      try {
        const list = await client.read<RunList>("/api/runs?limit=1");
        const row = (list.running ?? []).filter((r) => r.source === label && r.started_at_micros >= since).sort((a, b) => b.run - a.run)[0];
        if (row) return row;
      } catch {
        // Try again.
      }
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    return null;
  };

  const draft = (decision: string) =>
    act<Draft>({ name: "draft", decision }, "/api/draft-rule", { decision });

  /**
   * Saves a model key typed on the page in the local secret store (`key: null` removes
   * the saved one), then reads status again so the provider shows. The key goes no further
   * than the request.
   */
  const saveModelKey = async (provider: string, key: string | null): Promise<ActionOutcome<SavedKey>> => {
    let outcome: ActionOutcome<SavedKey>;
    try {
      outcome = await client.act<SavedKey>("/api/model-key", { provider, key });
    } catch (error) {
      if (!(error instanceof SignedOutError)) throw error;
      outcome = { ok: false, kind: "signed-out", message: SIGNED_OUT_LINE };
    }
    if (outcome.ok) await refreshStatus();
    return outcome;
  };

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
  // The chat the page waits on, so Stop can give it up.
  let chatAbort: AbortController | null = null;

  /**
   * Stops the agent answering the pending chat: the request is given up, which the
   * playground takes as the chat being cancelled before its next step, and `runs`, the
   * programs it is running now, are cancelled.
   */
  const stopChat = (runs: number[]) => {
    chatAbort?.abort();
    for (const run of runs) void client.act("/api/cancel", { run }).catch(() => undefined);
  };

  const chat = async (message: string, settings?: NewSessionSettings) => {
    const shown = snap.data.sessionId;
    const starting = settings !== undefined || shown === null;
    const body: Record<string, unknown> = { task: message };
    if (settings) {
      for (const [key, value] of Object.entries(settings)) if (value !== undefined) body[key] = value;
    } else if (shown !== null) {
      body.session = shown;
    }
    set({ pendingChat: { message, session: starting ? null : shown } });
    const abort = new AbortController();
    chatAbort = abort;
    window.addEventListener("beforeunload", warnBeforeLeaving);
    let polling = false;
    if (starting) {
      ownChat = null;
      followNewChat = true;
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
      const outcome = await act<AgentResult>({ name: "chat" }, "/api/agent", body, abort.signal);
      if (starting) {
        polling = false;
        // A chat that failed after its session started still made the session; it is
        // looked for once more so the failure shows where it happened.
        if (outcome.ok) {
          ownChat = outcome.result.session;
          known.add(ownChat);
        } else if (awaitingChat) {
          try {
            adoptOwnChat(await client.read<SessionList>("/api/sessions"));
          } catch {
            // Not listed; the page keeps the new session it was setting up.
          }
        }
        if (outcome.ok && followNewChat && ownChat && snap.data.sessionId !== ownChat) await openSession(ownChat);
      }
      return { outcome, session: starting ? (followNewChat ? ownChat : null) : shown };
    } finally {
      polling = false;
      awaitingChat = null;
      if (starting) followNewChat = false;
      if (chatAbort === abort) chatAbort = null;
      window.removeEventListener("beforeunload", warnBeforeLeaving);
      set({ pendingChat: null });
    }
  };

  // ---- the window ----

  const onHashChange = () => {
    const link = readLink(location.hash);
    if (link.code) {
      void signIn(location.hash);
      return;
    }
    // A run's link pasted into the page that is open: show that run.
    if (link.requested && snap.auth === "signed-in" && link.requested.run !== snap.requested?.run) {
      const requested = link.requested;
      set({ requested });
      void requestedSession(requested.run).then((id) => {
        if (id !== null && id !== snap.data.sessionId) void openSession(id);
      });
    }
  };
  // While the page is shown, what changed elsewhere shows within a few seconds.
  let liveTimer: ReturnType<typeof setInterval> | null = null;
  const livePoll = () => {
    if (snap.auth !== "signed-in" || document.visibilityState === "hidden") return;
    void refreshStatus();
    void refreshSessions();
  };
  // A blueprint saved in the editor shows when the developer comes back to the page.
  const onFocus = () => {
    if (snap.auth !== "signed-in") return;
    void refreshStatus();
    void refreshSessions();
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
      if (livePollMs > 0) liveTimer = setInterval(livePoll, livePollMs);
      await signIn(location.hash);
    },
    dispose() {
      feed?.stop();
      feed = null;
      generation += 1;
      awaitingChat = null;
      if (liveTimer) clearInterval(liveTimer);
      if (loadRetry) clearTimeout(loadRetry);
      window.removeEventListener("hashchange", onHashChange);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("beforeunload", warnBeforeLeaving);
      document.removeEventListener("visibilitychange", onVisibility);
    },

    /** Shows another session: stops the current feed and follows that one's. */
    openSession: (id: string) => {
      // The reader chose another session: a page chat still pending no longer pulls them back.
      followNewChat = false;
      if (snap.requested) set({ requested: null, requestedMissing: null });
      return openSession(id);
    },
    /** The reader moved on from a new chat still waiting for its first answer. */
    leaveNewChat: () => {
      followNewChat = false;
    },
    dismissNotice: () => {
      // A link to a run that isn't recorded is dropped with its notice, so a reload doesn't repeat it.
      if (snap.requestedMissing !== null) {
        history.replaceState(null, "", location.pathname + location.search);
        set({ requested: null });
      }
      set({ newSession: null, requestedMissing: null });
    },
    refreshSessions,
    /** Forgets the run a link asked for, once the reader moves elsewhere. */
    clearRequested: () => set({ requested: null, requestedMissing: null }),
    loadPayloads,

    loadExplain,
    loadVersion,
    loadVersions,
    loadSource,

    test,
    rerun,
    cancel,
    followUpInFlight,
    draft,
    saveModelKey,
    chat,
    stopChat,
  };
}

function dedupe(events: StoredEvent[]): StoredEvent[] {
  return events.filter((e, i) => i === 0 || e.session_seq !== events[i - 1].session_seq);
}

const SHOWN_KEY = "submilli-playground-shown-session";

/** The session this tab last showed, so a reload comes back to it. */
function readShown(): string | null {
  try {
    return sessionStorage.getItem(SHOWN_KEY);
  } catch {
    return null;
  }
}

function keepShown(id: string) {
  try {
    sessionStorage.setItem(SHOWN_KEY, id);
  } catch {
    // Storage is off; a reload opens the newest session.
  }
}
