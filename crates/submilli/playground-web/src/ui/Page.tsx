// The page, once signed in: the header, the session bar, the summary, the step bar, the
// three panes, and the dialogs over them. One `UiState` says how the reader is looking at
// the record; the wording in `../model` turns it and the store's data into what shows.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import type { AgentResult, DeclaredVariable, Draft, Show, Status } from "../api/types";
import { sourceKey } from "../api/types";
import { packageRules, blueprintDialog } from "../model/blueprint";
import { askBox, chatEmptyText, chatItems, noChat, runWord, selectedRun, type ChatItem } from "../model/chat";
import { codePane } from "../model/code";
import { capitalize, copyNotice, end, executionLabel, outcomeLabel, plainBy, sourceRunOf } from "../model/format";
import { expandAll, logPane, type Action, type RuleSpot } from "../model/log";
import { actionBlock, footer, keptValues, sessionBar, sessionRows, wayBack } from "../model/sessions";
import { billedName, checkSetup, initialSetup, INITIAL_SETUP, selectedSetupStatus, type SetupForm } from "../model/setup";
import { AT_START, byAgent, frame, nextKey, playFrom, prevKey, runsInFlight } from "../model/steps";
import { summary } from "../model/summary";
import { INITIAL_UI, type Dialog, type UiState } from "../model/ui";
import type { NewSessionSettings, PageStore, Snapshot } from "../store/store";
import { ChatPane } from "./ChatPane";
import { CodePane } from "./CodePane";
import { BlueprintView, Callout, ConfirmDialog, DraftDialog, PackagesDialog, SessionsDialog, ToastView, VarsDialog, type Toast } from "./Dialogs";
import { Values } from "./controls";
import { Header } from "./Header";
import { LogPane, type LogActions } from "./LogPane";
import { SessionBar } from "./SessionBar";
import { PaneTabs, type PaneName } from "./PaneTabs";
import { StepBar, Summary } from "./Summary";
import { useFollow } from "./useFollow";
import { STOPPED, UNREACHABLE } from "../api/client";
import { runHash } from "../api/login";
import {
  newSessionNotice,
  RECONNECTING,
  SESSION_LOADING,
  SOURCE_LOADING,
  WAITING,
  answering,
  BILLING_CANCELLED,
  billingAsk,
  blueprintRefused,
  chatFailed,
  chatStopped,
  cancellingRun,
  commandOf,
  confirmOf,
  draftRef,
  feedStopped,
  followUpDone,
  followUpSessionNotice,
  recheckSaid,
  gapNote,
  runEnded,
  notInForce,
  runMissing,
} from "./wording";

// How long a toast stays up, and how long after the pointer leaves it.
const TOAST_MS = 5000;
const TOAST_AFTER_HOVER_MS = 2000;
const PLAY_MS = 900;

/** Controls keep their own arrows, including caret movement in read-only text fields.
 * Playback buttons deliberately share the page's timeline shortcut. */
function ownsArrowKeys(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const control = target.closest("button, summary, a[href], input, textarea, select, [contenteditable]:not([contenteditable='false']), [role='button'], [role='tab'], [role='tablist'], [role='textbox'], [role='combobox'], [role='slider'], [role='spinbutton']");
  return !!control && !(control.matches("button") && control.closest(".pg-step"));
}

/** `askingIn` for a new session not started yet, told apart by when it was set up; never a session id. */
const freshBox = (started: string) => "#fresh " + started;

export function Page({ store, snap }: { store: PageStore; snap: Snapshot }) {
  const { data } = snap;
  const status = data.status;
  // A session started but not yet sent its first message lives only in the page, so the
  // tab keeps it: a reload comes back to it rather than to the latest session.
  const [ui, setUi] = useState<UiState>(() => {
    const fresh = readKept<UiState["fresh"]>(FRESH_KEY);
    return fresh && !snap.requested ? { ...INITIAL_UI, mode: "fresh", fresh } : restoreView(data.sessionId);
  });
  const patch = useCallback((p: Partial<UiState> | ((u: UiState) => Partial<UiState>)) => setUi((u) => ({ ...u, ...(typeof p === "function" ? p(u) : p) })), []);
  const [form, setForm] = useState<SetupForm>(INITIAL_SETUP);
  // The view New session was pressed from, so Back can return to it.
  const [setupFrom, setSetupFrom] = useState<UiState | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const [pending, setPending] = useState<Action | null>(null);
  const [rerunVariables, setRerunVariables] = useState<Record<string, string>>({});
  const [rerunError, setRerunError] = useState("");
  const [rerunRunning, setRerunRunning] = useState(false);
  const [draft, setDraft] = useState<Draft | null>(null);
  const composerId = ui.mode === "fresh" && ui.fresh ? freshBox(ui.fresh.started) : data.sessionId;
  const [chatText, setChatText] = useComposerDraft(composerId);
  const [activePane, setActivePane] = useState<PaneName>(() => readKept<PaneName>(PANE_KEY) ?? "chat");
  const [layout, setLayout] = useState<"split" | "focus">(() => readKept<"split" | "focus">(LAYOUT_KEY) ?? "split");
  useEffect(() => { keep(PANE_KEY, activePane); }, [activePane]);
  useEffect(() => { keep(LAYOUT_KEY, layout); }, [layout]);
  const [away, setAway] = useState({ chat: false, log: false });
  // A message the agent couldn't answer, shown where it was sent: in a session, or in the new
  // one (`null`). Kept for the tab, so a reload still says why the message has no answer.
  const [failure, setFailureState] = useState<Failure | null>(() => readKept<Failure>(FAILURE_KEY));
  const setFailure = (f: Failure | null) => {
    setFailureState(f);
    keep(FAILURE_KEY, f);
  };
  // What Claude Code would bill, waiting for the reader to confirm before the message goes.
  const [billing, setBilling] = useState<BillingAsk | null>(null);
  useEffect(() => {
    // A link or another session can be inspected without discarding the unsent setup.
    if (ui.mode === "fresh" && ui.fresh) keep(FRESH_KEY, ui.fresh.messages.length === 0 ? ui.fresh : null);
  }, [ui.mode, ui.fresh]);
  // The run Cancel was pressed for, until the playground confirms.
  const [cancelling, setCancelling] = useState<number | null>(null);
  // An action the page sent and is waiting on, so a second click doesn't send it again.
  const busy = useRef(false);
  const askInput = useRef<HTMLTextAreaElement>(null);
  // A row picked before it exists (a new rerun) comes into view once it does.
  const scrollTo = useRef<string | null>(null);
  // The new row of a test or rerun the page started, to take focus once it shows.
  const focusRow = useRef<string | null>(null);
  // Only an explicit cross-pane link requests focus. Feed updates and normal tab
  // navigation never set this request, so they leave the reader's focus alone.
  const paneFocus = useRef<{ pane: PaneName; row?: string } | null>(null);
  useLayoutEffect(() => {
    const request = paneFocus.current;
    if (!request) return;
    const panel = document.getElementById("pg-panel-" + request.pane);
    const row = request.row ? document.getElementById("pgrow-" + request.row)?.querySelector<HTMLElement>(".pg-event-head") : null;
    const destination = row ?? panel?.querySelector<HTMLElement>("h2");
    if (!destination) return;
    paneFocus.current = null;
    destination.focus({ preventScroll: true });
  });


  const viewedBlueprint = ui.mode === "setup" ? form.blueprint : ui.mode === "fresh" ? ui.fresh?.blueprint : ui.mode === "open" ? data.sessions.find((session) => session.session === data.sessionId)?.blueprint : undefined;
  const f = useMemo(() => frame(data.status && viewedBlueprint ? { ...data, status: selectedSetupStatus(data.status, viewedBlueprint) } : data, ui), [data, ui, viewedBlueprint]);
  const blocked = actionBlock(f);

  // ---- toasts and copying

  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Where focus was when the toast came up, for Dismiss to give it back to.
  const toastFrom = useRef<Element | null>(null);
  const showToast = useCallback((t: Toast, ms = TOAST_MS) => {
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastFrom.current = document.activeElement;
    setToast(t);
    if (t.auto) toastTimer.current = setTimeout(() => setToast((now) => (now === t ? null : now)), ms);
  }, []);
  useEffect(() => () => {
    if (toastTimer.current) clearTimeout(toastTimer.current);
  }, []);

  const copy = useCallback(
    async (text: string, what: "reference" | "command") => {
      let ok = false;
      try {
        await navigator.clipboard.writeText(text);
        ok = true;
      } catch {
        ok = false;
      }
      showToast(copyNotice(text, what, ok));
    },
    [showToast],
  );

  // ---- dialogs, and giving focus back to what opened them

  const opener = useRef<Element | null>(null);
  const openDlg = (dlg: Dialog, extra: Partial<UiState> = {}) => {
    opener.current = document.activeElement;
    patch({ dlg, ...extra });
  };
  const closeDlg = () => patch({ dlg: null });
  const lastDlg = useRef<Dialog>(null);
  useEffect(() => {
    if (lastDlg.current && !ui.dlg) {
      const back = opener.current;
      opener.current = null;
      if (back instanceof HTMLElement && back.isConnected) back.focus();
    }
    lastDlg.current = ui.dlg;
  }, [ui.dlg]);

  // ---- the session shown

  // Another session shown starts the reader over, at its latest event. One being set up
  // stays as it is: its Back now goes to the session shown.
  const shown = useRef(data.sessionId);
  const sessionChanged = shown.current !== data.sessionId;
  // The ask box the reader is at now: a new session's, or the shown session's. A message
  // that failed goes back only into the box it was sent from.
  const askingIn = useRef<string | null>(null);
  askingIn.current = ui.mode === "fresh" && ui.fresh ? freshBox(ui.fresh.started) : ui.mode === "open" ? data.sessionId : null;
  // What to show in the next session opened, as when stepping through a rerun opens its session at the rerun.
  const showNext = useRef<Partial<UiState> | null>(null);
  // A test's or rerun's own session, opened from its row: where the reader came from, so they can go back.
  const [stepping, setStepping] = useState<{ to: string; from: string | null; run: number; source: number | null; kind: string } | null>(null);
  // A run the link asked for is picked and opened once its session shows and its record is loaded.
  const requestedDone = useRef<number | null>(null);
  // The row the link picked; once the reader picks another, the link no longer says what's shown.
  const linkSel = useRef<string | null>(null);
  // The address stops naming a run, so a reload comes back to what's shown instead.
  const forgetRunLink = () => {
    if (snap.requested) store.clearRequested();
    if (location.hash.startsWith("#run=")) history.replaceState(null, "", location.pathname + location.search);
  };
  useEffect(() => {
    if (shown.current === data.sessionId) return;
    shown.current = data.sessionId;
    const next = showNext.current ?? {};
    showNext.current = null;
    // Stepping through a rerun is left once another session than its own shows.
    if (stepping && data.sessionId !== stepping.to) setStepping(null);
    // A dialog the reader opened meanwhile stays open, as does their summary preference.
    setUi((u) => (u.mode === "setup" ? u : { ...restoreView(data.sessionId), summaryOpen: u.summaryOpen, dlg: u.dlg, ...next }));
    setSetupFrom(ui.mode === "setup" && data.sessionId !== null ? INITIAL_UI : null);
    // A run's link names one session; once another shows, it no longer applies. One still
    // waiting for its session to show stays.
    if (!snap.requested || requestedDone.current === snap.requested.run) forgetRunLink();
  }, [data.sessionId]);

  // Persist inspection only. Reload never resumes playback or sends an action.
  useEffect(() => {
    if (sessionChanged || ui.mode !== "open" || !data.sessionId) return;
    keep(viewKey(data.sessionId), { cursor: ui.cursor, sel: ui.sel, open: ui.open, followOpen: ui.followOpen, checksOpen: ui.checksOpen, filter: ui.filter, asked: ui.asked, program: ui.program, summaryOpen: ui.summaryOpen });
  }, [data.sessionId, ui, sessionChanged]);

  useEffect(() => {
    if (linkSel.current !== null && ui.sel !== null && ui.sel !== linkSel.current) {
      linkSel.current = null;
      forgetRunLink();
    }
  }, [ui.sel]);
  useEffect(() => {
    const req = snap.requested;
    if (!req || requestedDone.current === req.run || snap.sessionLoading) return;
    const show = data.runs[req.run];
    if (!show) return;
    const source = sourceRunOf(show);
    const home = source !== null ? data.runs[source]?.session : show.session;
    if (!home || home !== data.sessionId) return;
    requestedDone.current = req.run;
    // The run the link names shows whatever the log was filtered to.
    patch({ mode: "open", fresh: null, cursor: null, playing: false, program: null, filter: "all" });
    setActivePane("log");
    if (location.hash !== runHash(req)) history.replaceState(null, "", location.pathname + location.search + runHash(req));
    if (source !== null) {
      patch((u) => ({ sel: "f" + req.run, open: { ...u.open, ["r" + source]: true, ["f" + req.run]: true }, followOpen: { ...u.followOpen, [source]: true } }));
    } else if (req.decision !== null) {
      // A repeated check is listed under the first of its group, so the link opens that row.
      const asked = req.run + "." + req.decision;
      const ref = show.decisions.find((d) => d.refs.includes(asked))?.refs[0] ?? asked;
      patch((u) => ({
        sel: "d" + ref,
        asked: { sel: "d" + ref, ref: asked },
        open: { ...u.open, ["r" + req.run]: true, ["d" + ref]: true },
        checksOpen: { ...u.checksOpen, [req.run]: true },
      }));
    } else {
      patch((u) => ({ sel: "r" + req.run, open: { ...u.open, ["r" + req.run]: true } }));
    }
    const group = req.decision !== null ? show.decisions.find((d) => d.refs.includes(req.run + "." + req.decision))?.refs[0] : undefined;
    scrollTo.current = source !== null ? "f" + req.run : req.decision !== null ? "d" + (group ?? req.run + "." + req.decision) : "r" + req.run;
    linkSel.current = scrollTo.current;
  }, [snap.requested, snap.sessionLoading, data.runs, data.sessionId, patch]);

  // An opened run's checks are explained, and its blueprint version's text is read, so
  // its rows can name the nearest rule and the line it's on.
  useEffect(() => {
    for (const [id, open] of Object.entries(ui.open)) {
      if (!open || !id.startsWith("r")) continue;
      const show = data.runs[Number(id.slice(1))];
      if (!show) continue;
      for (const d of show.decisions) void store.loadExplain(d.refs[0]);
      void store.loadVersion(Number(show.blueprint_version));
    }
  }, [ui.open, data.runs, store]);

  // At a package step, its source is read.
  const atPackage = f.atCall && f.atCall.kind === "package" ? (f.session.calls[f.atCall.run] || []).find((c) => c.index === f.atCall?.index)?.pkg ?? null : null;
  useEffect(() => {
    if (atPackage) void store.loadSource(atPackage.package, atPackage.module);
  }, [atPackage?.package, atPackage?.module, store]);

  // The blueprint dialog lists the versions and shows one; the packages dialog counts rules in force.
  useEffect(() => {
    if (ui.dlg === "bp") {
      void store.loadVersions();
      const v = ui.bpV ?? f.inForce;
      if (v !== null) void store.loadVersion(v);
    }
    if (ui.dlg === "pkgs" && f.inForce !== null) void store.loadVersion(f.inForce);
  }, [ui.dlg, ui.bpV, f.inForce, store]);

  // A picked row comes into view; one that doesn't exist yet does once it does.
  useEffect(() => {
    if (ui.sel) scrollTo.current = ui.sel;
  }, [ui.sel]);
  useEffect(() => {
    const id = scrollTo.current;
    const el = id ? document.getElementById("pgrow-" + id) : null;
    if (!el) return;
    scrollTo.current = null;
    // A row taller than the log shows from its head, not its end.
    const pane = el.closest(".pg-log");
    el.scrollIntoView?.({ block: pane && el.getBoundingClientRect().height > pane.clientHeight ? "start" : "nearest" });
    // Focus goes on to the new row, unless the reader has moved it out of the log meanwhile.
    if (focusRow.current === id) {
      focusRow.current = null;
      const at = document.activeElement;
      if (!at || at === document.body || el.closest(".pg-log")?.contains(at)) el.querySelector<HTMLElement>(".pg-event-head")?.focus();
    }
  });

  // ---- stepping

  const steps = f.steps;
  const go = (cursor: string | null) => patch({ cursor, playing: false, sel: null, program: null });
  const prev = () => go(prevKey(steps, ui.cursor));
  const next = () => go(nextKey(steps, ui.cursor));
  const chatFollow = useFollow<HTMLDivElement>(
    f.vis.length + (snap.pendingChat ? 1 : 0),
    useCallback((a: boolean) => setAway((w) => ({ ...w, chat: a })), []),
    data.sessionId,
    data.sessionId ? "submilli-playground-scroll:chat:" + data.sessionId : undefined,
  );
  // New runs below are something to follow; a rerun opening under a run above isn't.
  const logFollow = useFollow<HTMLDivElement>(
    f.vis.length + f.running.length,
    useCallback((a: boolean) => setAway((w) => ({ ...w, log: a })), []),
    data.sessionId,
    data.sessionId ? "submilli-playground-scroll:log:" + data.sessionId : undefined,
  );
  const toLatest = () => {
    patch({ cursor: null, playing: false });
    chatFollow.toEnd();
    logFollow.toEnd();
  };

  const replaySteps = useRef(steps);
  useEffect(() => {
    replaySteps.current = steps;
  }, [steps]);
  useEffect(() => {
    if (!ui.playing) return;
    // Refreshes and other UI changes must not restart the playback clock.
    const timer = setInterval(() => {
      setUi((u) => {
        const n = nextKey(replaySteps.current, u.cursor);
        return n === null ? { ...u, cursor: null, playing: false } : { ...u, cursor: n };
      });
    }, PLAY_MS);
    return () => clearInterval(timer);
  }, [ui.playing]);
  const togglePlay = () => {
    if (ui.playing) patch({ playing: false });
    else patch({ playing: true, cursor: playFrom(steps, ui.cursor), sel: null, program: null });
  };

  // ---- actions

  // The live run Cancel would stop: one in flight in the shown session's feed.
  const inFlight = ui.mode === "open" ? runsInFlight(data.events).filter((n) => !data.runs[n]) : [];
  const cancelRun = inFlight.length ? inFlight[inFlight.length - 1] : null;

  // The playground's own words, as a sentence.
  const refused = (message: string) => showToast({ head: capitalize(message), auto: false });

  // The test or rerun running now, for the Cancel on its notice.
  const followUp = useRef<{ label: "test" | "rerun"; since: number; source: number } | null>(null);
  const rerunContext = (run: number): { variables: DeclaredVariable[]; values: Record<string, string> } | string => {
    const record = data.runs[run];
    const session = data.sessions.find((item) => item.session === record?.session);
    const name = record?.blueprint ?? session?.blueprint;
    if (!name) return "The recorded blueprint for run " + run + " is unavailable. Reload its details before rerunning.";
    const selected = name === status?.blueprint.name ? status : status?.blueprints?.find((item) => item.name === name);
    if (!selected) return "Blueprint " + name + " is not available. Reload its context before rerunning.";
    return { variables: selected.declared_variables, values: record?.variables ?? session?.variables ?? {} };
  };
  const act = async (action: Action, confirmed = false) => {
    if (blocked || busy.current) return;
    if (!confirmed && confirmOf(action)) {
      if (action.kind === "rerun") {
        const context = rerunContext(action.run);
        if (typeof context === "string") return refused(context);
        setRerunVariables(Object.fromEntries(context.variables.map(({ name }) => [name, context.values[name] ?? ""])));
        setRerunError("");
      }
      setPending(action);
      openDlg("confirm");
      return;
    }
    if (action.kind === "rerun") {
      const context = rerunContext(action.run);
      if (typeof context === "string") return setRerunError(context);
      const missing = context.variables.filter(({ name, required }) => required && !rerunVariables[name]);
      if (missing.length) return setRerunError("Enter " + missing.map(({ name }) => name).join(", ") + " before rerunning.");
    }
    busy.current = true;
    try {
      if (action.kind === "draft") {
        const outcome = await store.draft(action.decision);
        if (!outcome.ok) return refused(outcome.message);
        setDraft(outcome.result);
        openDlg("draft");
        return;
      }
      if (action.kind === "recheck") {
        const outcome = await store.recheck(action.run);
        if (!outcome.ok) return refused(outcome.message);
        return showToast({ ...recheckSaid(outcome.result), auto: false });
      }
      const startedIn = shown.current;
      if (action.kind === "rerun") {
        setRerunError("");
        setRerunRunning(true);
      }
      showToast({ head: "Running:", body: commandOf(action), auto: false, cancelable: true, watchable: true });
      // Runs started from now on are this action's, for its Cancel.
      followUp.current = { label: action.kind, since: Date.now() * 1000 - 2_000_000, source: action.run };
      const outcome = await (action.kind === "test" ? store.test(action.run, action.mode) : store.rerun(action.run, rerunVariables)).finally(() => {
        followUp.current = null;
      });
      if (!outcome.ok) {
        if (action.kind === "rerun") {
          setRerunError(outcome.message);
          return showToast({ head: "Couldn’t rerun run " + action.run, body: capitalize(outcome.message), auto: false });
        }
        return refused(outcome.message);
      }
      if (action.kind === "rerun") {
        setPending(null);
        patch((u) => u.dlg === "confirm" ? { dlg: null } : {});
      }
      const made: Show = outcome.result;
      const source = action.run;
      // Watched in its own session meanwhile: the reader is there, so only the notice says how it ended.
      if (shown.current !== startedIn) return showToast({ head: followUpDone(action.kind, source, made.run, end(made.outcome), made.blueprint_version), auto: true });
      // The new run shows under the run it followed, opened so it can be seen.
      // Keep the historical event selected while exposing the new comparison below it.
      patch((u) => ({
        open: { ...u.open, ["r" + source]: true, ["f" + made.run]: true },
        followOpen: { ...u.followOpen, [source]: true },
      }));
      showToast({ head: followUpDone(action.kind, source, made.run, end(made.outcome), made.blueprint_version), auto: true });
    } finally {
      busy.current = false;
      setRerunRunning(false);
    }
  };

  const confirmGo = () => {
    const action = pending;
    if (action?.kind !== "rerun") {
      setPending(null);
      closeDlg();
    }
    if (action) void act(action, true);
  };

  // Opens a session the reader picked. The New session form stays open while another
  // session shows behind it, so picking one leaves the form first.
  const openPicked = (id: string) => {
    if (ui.mode === "fresh") store.leaveNewChat();
    setUi((u) => ({ ...restoreView(id), summaryOpen: u.summaryOpen }));
    setSetupFrom(null);
    if (id !== data.sessionId) void store.openSession(id);
  };

  const stepThrough = async (session: string, run: number, kind?: "test" | "rerun", from?: number) => {
    const made = data.runs[run];
    const source = made ? sourceRunOf(made) : from ?? null;
    // Listed first, so the session opens with its runs.
    await store.refreshSessions();
    setStepping({ to: session, from: wayBack(stepping, data.sessionId, session), run, source, kind: kind ?? (made?.rerun_of ? "rerun" : "test") });
    showNext.current = { sel: "r" + run, open: { ["r" + run]: true } };
    void store.openSession(session);
  };
  const stepBack = () => {
    const back = stepping;
    if (!back?.from) return;
    // Back where the row was: under its source run, opened.
    if (back.source !== null) showNext.current = { sel: "f" + back.run, open: { ["r" + back.source]: true, ["f" + back.run]: true }, followOpen: { [back.source]: true } };
    scrollTo.current = "f" + back.run;
    void store.openSession(back.from);
  };

  // Opens the running test's or rerun's own session, to watch its calls as they come.
  const watchFollowUp = async () => {
    const going = followUp.current;
    if (!going) return;
    const row = await store.followUpInFlight(going.label, going.since);
    if (row?.session) await stepThrough(row.session, row.run, going.label, going.source);
  };
  const cancelFollowUp = async () => {
    const going = followUp.current;
    if (!going) return;
    const row = await store.followUpInFlight(going.label, going.since);
    // Finished meanwhile: what it did shows as it ends.
    if (row === null || !followUp.current) return;
    const run = row.run;
    showToast({ head: cancellingRun(run), auto: false, cancelling: run });
    const outcome = await store.cancel(run);
    if (!outcome.ok && followUp.current) refused(outcome.message);
  };

  // Cancel waits until the run has ended, not just until the playground took the request.
  const cancel = async (run: number) => {
    if (cancelling === run) return;
    setCancelling(run);
    const outcome = await store.cancel(run);
    if (!outcome.ok) {
      setCancelling(null);
      return refused(outcome.message);
    }
    showToast({ head: cancellingRun(run), auto: true, cancelling: run });
  };
  useEffect(() => {
    if (cancelling === null || runsInFlight(data.events).includes(cancelling)) return;
    const run = cancelling;
    setCancelling(null);
    // The notice that it's cancelling gives way to how it ended.
    if (toast?.cancelling === run) showToast({ head: runEnded(run, runWord(f, run)), auto: true });
  }, [data.events, cancelling]);

  // ---- chat and new sessions

  const newSession = () => {
    if (!status) return;
    // A new chat still waiting for its first answer stays where it is; the page won't pull back to it.
    if (ui.mode === "fresh") store.leaveNewChat();
    // Opened from the sessions dialog, focus goes to the form, not back to Sessions.
    opener.current = null;
    // With no session yet, there is nothing to go back to.
    setSetupFrom(ui.mode === "setup" ? setupFrom : data.sessionId !== null || ui.mode === "fresh" ? ui : null);
    forgetRunLink();
    // Starting a new session resets the summary to its default open state.
    setUi(() => ({ ...INITIAL_UI, mode: "setup" }));
    setForm(initialSetup(status, data.sessions));
  };
  const goBack = () => {
    if (setupFrom) {
      setUi({ ...setupFrom, dlg: null });
    }
    setSetupFrom(null);
    // The form and its Back are gone; the session is back, so its ask box takes focus.
    setTimeout(() => askInput.current?.focus(), 0);
  };
  const startSession = () => {
    if (!status) return;
    const check = checkSetup(status, form);
    if (check.cantStart) return;
    setSetupFrom(null);
    setFailure(null);
    patch({
      mode: "fresh",
      fresh: { blueprint: form.blueprint || status.blueprint.name, variables: check.variables, agent: check.model, settings: check.settings, started: new Date().toISOString(), messages: [] },
    });
    // The next thing to do is say what the agent should do.
    setTimeout(() => askInput.current?.focus(), 0);
  };

  // Back in the box, above the next message if the reader has started one there meanwhile
  // (unless that is this one, written again).
  const keepToSendAgain = (text: string) =>
    setChatText((now) => (!now.trim() ? text : now.trim() === text ? now : `${text}\n\n${now}`));

  /** Sends the box's message; `confirmBilling` confirms what Claude Code bills, once asked. */
  const send = async (confirmBilling?: string) => {
    const text = chatText.trim();
    // One message at a time: a second while the first is unanswered would start another chat.
    if (!text || snap.pendingChat) return;
    setChatText("");
    setFailure(null);
    setBilling(null);
    const fresh = ui.mode === "fresh" ? ui.fresh : null;
    if (fresh) {
      const sentFrom = freshBox(fresh.started);
      patch({ fresh: { ...fresh, messages: [...fresh.messages, text] } });
      const settings: NewSessionSettings = {
        blueprint: fresh.blueprint,
        model: fresh.agent,
        variables: fresh.variables,
        effort: fresh.settings?.effort,
        max_output: fresh.settings?.max_output,
        max_budget_usd: fresh.settings?.max_budget_usd,
        provider_settings: fresh.settings?.provider,
      };
      const { outcome, session } = await store.chat(text, settings, confirmBilling);
      if (!outcome.ok) {
        // The failure shows where the message went: in the session the playground made
        // for it, or, when none was made, in the new session still being set up.
        const stopped = outcome.kind === STOPPED;
        notAnswered(outcome, session, session !== null);
        if (session === null) setUi((u) => (u.mode === "fresh" && u.fresh ? { ...u, fresh: { ...u.fresh, messages: u.fresh.messages.filter((m) => m !== text) } } : u));
        // Kept to send again in the box it was sent from, or in the session it started,
        // unless it was stopped there, which has it already.
        const alreadyInSession = stopped && session !== null;
        const atItsBox = askingIn.current === sentFrom || (session !== null && askingIn.current === session);
        if (!alreadyInSession && atItsBox) keepToSendAgain(text);
        return;
      }
      // The answer can come before the session is listed; it is shown now either way.
      setUi((u) => (u.mode === "fresh" ? { ...INITIAL_UI, summaryOpen: u.summaryOpen, dlg: u.dlg } : u));
      stoppedShort(outcome.result);
      return;
    }
    const { outcome, session } = await store.chat(text, undefined, confirmBilling);
    if (outcome.ok) stoppedShort(outcome.result);
    else {
      const stopped = outcome.kind === STOPPED;
      notAnswered(outcome, session, true);
      // Kept for the reader to send again, in the session it was written for; a stopped
      // message is already in the session.
      if (session === askingIn.current && !stopped) keepToSendAgain(text);
    }
  };

  // A message not answered asks to confirm billing, or says why, where it was sent.
  const notAnswered = (outcome: { kind: string; message: string }, session: string | null, started: boolean) => {
    const asked = billingAsked(outcome, status);
    if (asked) setBilling({ ...asked, session });
    else setFailure({ session, message: outcome.message, kind: outcome.kind, started, stopped: outcome.kind === STOPPED });
  };

  // An answer cut short by a limit says so, in the session it stopped in.
  const stoppedShort = (result: AgentResult) => {
    if (result.stopped && result.stopped !== "answered") setFailure({ session: result.session, message: "", started: true, stopped: true, reason: result.stopped });
  };

  // ---- the keyboard

  const onKey = (ev: KeyboardEvent) => {
    if (ev.key === "Escape") {
      if (ui.dlg) {
        closeDlg();
        setPending(null);
      } else if (toast) dismissToast();
      return;
    }
    if (ev.key !== "ArrowLeft" && ev.key !== "ArrowRight") return;
    if (ui.mode !== "open" || ui.dlg || ev.altKey || ev.ctrlKey || ev.metaKey || ev.defaultPrevented || ownsArrowKeys(ev.target)) return;
    ev.preventDefault();
    if (ev.key === "ArrowLeft" && !f.bar.atStart) prev();
    if (ev.key === "ArrowRight" && !f.bar.atEnd) next();
  };
  // The notice's button goes with it, so focus goes on to the ask box.
  const dismissNotice = () => {
    store.dismissNotice();
    setTimeout(() => askInput.current?.focus({ preventScroll: true }), 0);
  };
  const dismissToast = () => {
    setToast(null);
    const back = toastFrom.current;
    toastFrom.current = null;
    // Focus goes back without moving the page: the reader may have scrolled away since.
    if (back instanceof HTMLElement && back.isConnected && back !== document.body) back.focus({ preventScroll: true });
  };
  const keyRef = useRef(onKey);
  keyRef.current = onKey;
  useEffect(() => {
    const listener = (ev: KeyboardEvent) => keyRef.current(ev);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
  if (!status) return null;

  // ---- what shows

  const bar = sessionBar(f);
  const chat: ChatItem[] = chatItems(f);
  // A message sent in the shown session waits here until the feed brings the answer.
  const waitingHere = !!snap.pendingChat && ui.mode === "open" && snap.pendingChat.session === data.sessionId;
  if (waitingHere && !f.stepping) chat.push({ kind: "wait", text: WAITING });
  const failedHere = failure && (ui.mode === "fresh" ? failure.session === null : ui.mode === "open" && failure.session === data.sessionId);
  if (failure && failedHere && !f.stepping)
    chat.push(failure.stopped ? { kind: "stopped", ...chatStopped(failure.started, failure.reason) } : { kind: "failed", ...chatFailed(failure.message, failure.started, failure.kind) });
  const billingHere = billing && (ui.mode === "fresh" ? billing.session === null : ui.mode === "open" && billing.session === data.sessionId) && !f.stepping ? billing : null;
  const box = askBox(f);
  const backTo = stepping && stepping.from && stepping.from !== data.sessionId && data.sessionId === stepping.to ? stepping.from.slice(0, 8) : "";
  const askState = backTo && box.none ? { ...box, none: { ...box.none, back: backTo } } : snap.unreachable
    ? { ...box, disabled: true, primary: false, reason: UNREACHABLE.split(". ")[0] + "." }
    : snap.pendingChat && !box.disabled
      ? { ...box, disabled: true, primary: false, reason: answering(waitingHere || ui.mode === "fresh" ? null : snap.pendingChat.session ?? "new"), stop: waitingHere || ui.mode === "fresh", typeAhead: true }
      : box;
  const pane = codePane(f);
  const srcKey = atPackage ? sourceKey(atPackage.package, atPackage.module) : null;
  const srcError = srcKey ? snap.sourceErrors[srcKey] : undefined;
  const srcLoading = srcKey && !data.sources[srcKey] && !srcError ? SOURCE_LOADING : "";
  const log = logPane(f);
  const gaps = ui.mode === "open" ? data.events.flatMap((e) => ("gap" in e.body ? [gapNote(e.run, e.body.gap)] : [])) : [];
  const rows = sessionRows(f);
  const contextStatus = f.data.status ?? status;
  const refusedEdit = contextStatus.blueprint_status.refused;
  const kept = keptValues(f);
  const bp = blueprintDialog(f);
  const versionInForce = f.inForce;
  const projectHistoryVersion = (status.blueprints?.length ?? 0) > 1 && bp.version !== versionInForce;
  const bpView = projectHistoryVersion ? { ...bp, title: "Project blueprint history · v" + bp.version, summary: bp.summary.replace(" (in force)", ""), versions: bp.versions.map((version) => versionInForce === null ? { ...version, name: version.name.replace(" · in force", "") } : version) } : bp;

  const pickRun = (id: string, destination: "log" | "code") => {
    paneFocus.current = { pane: destination, row: destination === "log" ? id : undefined };
    patch((u) => ({ sel: id, playing: false, program: null, open: { ...u.open, [id]: true } }));
    setActivePane(destination);
  };
  const selected = selectedRun(ui.sel) ?? f.atCall?.run ?? (f.ended.length ? f.ended[f.ended.length - 1] : null);
  const selectedRecord = selected !== null ? data.runs[selected] : null;
  const selectedIsFollowUp = !!(selectedRecord?.test || selectedRecord?.rerun_of);
  const selectedOutcome = selectedRecord ? (f.stepping ? "Recorded outcome: " : "") + outcomeLabel(end(selectedRecord.outcome)) : "";
  const logActions: LogActions = {
    blocked,
    act: (a) => void act(a),
    copy: (t, w) => void copy(t, w),
    toggle: (id) => {
      const row = log.rows.flatMap((run) => [run, ...run.decisions, ...run.follow]).find((entry) => entry.id === id);
      patch((u) => ({ open: { ...u.open, [id]: id.startsWith("d") || u.sel === id ? !row?.open : true }, sel: id, program: null, playing: false }));
    },
    toggleChecks: (run, open) => patch((u) => ({ checksOpen: { ...u.checksOpen, [run]: open } })),
    toggleFollow: (run, open) => patch((u) => ({ followOpen: { ...u.followOpen, [run]: open } })),
    showLine: (id) => pickRun(id, "code"),
    openTried: (run) => {
      scrollTo.current = "f" + run;
      patch((u) => ({ open: { ...u.open, ["f" + run]: true }, sel: "f" + run }));
    },
    inspect: (spot: RuleSpot) => openDlg("bp", { bpV: spot.version, bpFocus: { line: spot.line, label: spot.label } }),
    cancel: (run) => void cancel(run),
    showAll: (run) => {
      patch((u) => ({ whole: { ...u.whole, [run]: true } }));
      void store.loadPayloads(run);
    },
    showLess: (run) => patch((u) => ({ whole: { ...u.whole, [run]: false } })),
    stepThrough: (session, run) => void stepThrough(session, run),
  };

  const inForceText = versionInForce !== null ? data.versions[versionInForce]?.text : undefined;
  const packages = contextStatus.packages.map((p) => ({ name: p.name, rules: inForceText ? packageRules(inForceText, p.name) : null }));

  const feedLine = snap.feed === "reconnecting" && !snap.unreachable ? RECONNECTING : "";
  const confirm = pending ? confirmOf(pending, rerunVariables) : null;
  const pendingRerun = pending?.kind === "rerun" ? rerunContext(pending.run) : null;
  const hideWorkbench = ui.mode === "setup" || (snap.noSessions && ui.mode === "open");

  return (
    <div className="pg">
      <Header
        names={f.names}
        status={contextStatus}
        inForce={versionInForce}
        setup={ui.mode === "setup"}
        noSessions={snap.noSessions && ui.mode === "open"}
        onBlueprint={() => openDlg("bp", { bpV: null, bpFocus: null })}
        onPackages={() => openDlg("pkgs")}
        onSessions={() => {
          // The list is read again as it opens, so a session started a moment ago is in it.
          void store.refreshSessions();
          openDlg("sessions");
        }}
        onBack={ui.mode === "setup" && setupFrom ? goBack : null}
        onNewSession={newSession}
      />

      {!(snap.noSessions && ui.mode === "open") && <SessionBar
        bar={bar && (snap.unreachable || snap.feed === "reconnecting" || snap.feedError) ? { ...bar, live: false } : bar}
        task={ui.mode === "fresh" ? "New session" : f.info?.untrusted?.task}
        setup={ui.mode === "setup"}
        status={contextStatus}
        form={form}
        cancelRun={cancelRun}
        cancelling={cancelling !== null && cancelling === cancelRun}
        onForm={setForm}
        onStart={startSession}
        onBlueprint={() => openDlg("bp", { bpV: contextStatus.blueprint_status.version, bpFocus: null })}
        onPackages={() => openDlg("pkgs")}
        keys={{ save: store.saveModelKey, done: (notice) => showToast({ head: notice, auto: true }) }}
        onCopy={() =>
          bar && bar.copyText
            ? void copy(bar.copyText, "reference")
            : showToast({ head: "No reference yet", body: "This session starts on the playground when you send its first message.", auto: true })
        }
        onAllVars={() => openDlg("allvars")}
        onCancel={(run) => void cancel(run)}
        copy={(t, w) => void copy(t, w)}
      />}

      {snap.noSessions && ui.mode === "open" && (
        <section className="pg-empty-start" aria-labelledby="pg-empty-start-title">
          <h1 id="pg-empty-start-title">Start a playground session</h1>
          <p>Use New session above to choose an agent model and application context.</p>
        </section>
      )}

      {snap.unreachable && <Callout text={UNREACHABLE} tone="bad" />}
      {refusedEdit && <Callout text={blueprintRefused(f.names.blueprint, refusedEdit, versionInForce)} tone="bad" />}
      {snap.feedError && <Callout text={feedStopped()} tone="bad" />}
      {backTo && stepping && (
        <Callout text={followUpSessionNotice(stepping.kind, stepping.run, stepping.source)}>
          {/* A link, as the other notices' actions are: the chat holds the button. */}
          <button className="pg-link" onClick={stepBack}>
            Back to session {backTo}
          </button>
        </Callout>
      )}
      {kept && (
        <details className="pg-context-note"><summary>Application context differs from current settings</summary><div className="pg-callout" role="note">
          <Values v={kept} />
          <button className="pg-link" onClick={newSession}>
            Start a new session to use them
          </button>
        </div></details>
      )}
      {snap.requestedMissing !== null && (
        <Callout text={runMissing(snap.requestedMissing)}>
          <button className="pg-link" onClick={dismissNotice}>
            Dismiss
          </button>
        </Callout>
      )}
      {snap.newSession && (
        <Callout text={newSessionNotice(snap.newSession, data.sessions.find((s) => s.session === snap.newSession)?.untrusted?.task)}>
          <button className="pg-link" onClick={() => snap.newSession && openPicked(snap.newSession)}>
            Open it
          </button>
          <button className="pg-link" onClick={dismissNotice}>
            Dismiss
          </button>
        </Callout>
      )}

      {!hideWorkbench && <div className="pg-inspection-context" aria-label="Selected execution context">
        <strong>{ui.mode === "fresh" ? "New session · ready for its first message" : selected !== null ? `Viewing Run ${selected}` : "No run selected"}</strong>
        {selectedRecord && <span>{selectedIsFollowUp ? executionLabel(selectedRecord) : selectedOutcome}</span>}
        {selectedIsFollowUp && <span>{selectedOutcome}</span>}
        {f.stepping && <span>Recorded playback</span>}
        <button className="pg-link" onClick={() => openDlg("bp", { bpV: selectedRecord ? Number(selectedRecord.blueprint_version) : null, bpFocus: null })}>
          {selectedRecord ? `Recorded with v${selectedRecord.blueprint_version}` : "Recorded blueprint unavailable"}
        </button>
      </div>}
      {!hideWorkbench && <Summary s={summary(f)} open={ui.summaryOpen} onToggle={() => patch({ summaryOpen: !ui.summaryOpen })} />}

      {ui.mode === "open" && !snap.noSessions && (
        <StepBar
          bar={f.bar}
          playing={ui.playing}
          behind={f.stepping || away.chat || away.log}
          onStart={() => go(steps.length ? AT_START : null)}
          onPrev={prev}
          onPlay={togglePlay}
          onNext={next}
          onLatest={toLatest}
        />
      )}

      {!hideWorkbench && <div className="pg-workbench-tools">
        <PaneTabs active={activePane} onChange={setActivePane} />
        <button className="pg-link pg-layout-toggle" aria-pressed={layout === "focus"} onClick={() => setLayout(layout === "split" ? "focus" : "split")}>{layout === "split" ? "Focus one pane" : "Show all panes"}</button>
      </div>}
      {!hideWorkbench && <main className="pg-app" data-pane={activePane} data-layout={layout}>
        <ChatPane
          items={chat}
          empty={ui.mode === "open" && (snap.sessionLoading || snap.feed === "connecting") ? SESSION_LOADING : chatEmptyText(f, stepping && data.sessionId === stepping.to ? stepping.source : null)}
          box={snap.noSessions && ui.mode === "open" ? noChat("No session yet.") : askState}
          text={chatText}
          scrollRef={chatFollow.ref}
          inputRef={askInput}
          onScroll={chatFollow.onScroll}
          onText={setChatText}
          onSend={() => void send()}
          confirm={billingHere ? {
            ...(billingHere.cancelled ? { text: BILLING_CANCELLED, note: "" } : billingAsk(billingHere.billed, billingHere.changed)),
            open: !billingHere.cancelled,
            onConfirm: () => void send(billingHere.confirmation),
            onCancel: () => setBilling({ ...billingHere, cancelled: true }),
          } : undefined}
          onStop={() => store.stopChat(f.running.filter((n) => byAgent(f, n)))}
          onBack={stepBack}
          onPick={(run) => pickRun("r" + run, "log")}
        />
        <CodePane
          pane={pane}
          sourceLoading={srcLoading}
          sourceError={srcError ? plainBy(srcError.message) : ""}
          sel={ui.sel}
          program={ui.program}
          cursor={ui.cursor}
          onCopy={(t) => void copy(t, "reference")}
        />
        <LogPane
          log={log}
          filter={ui.filter}
          gaps={gaps}
          cancelRun={log.runningRun !== null && log.runningRun === cancelRun ? cancelRun : null}
          cancelling={cancelling !== null && cancelling === cancelRun}
          scrollRef={logFollow.ref}
          onScroll={logFollow.onScroll}
          onFilter={(filter) => patch({ filter })}
          onToggleAll={() => patch(expandAll(log, ui))}
          a={logActions}
        />
      </main>}

      {!hideWorkbench && <footer className="pg-foot">
        <span>{failedHere && !failure?.reason && !f.stepping && !waitingHere ? "Live session · the agent couldn’t answer the last message" : snap.feedError ? "Live updates stopped" : footer(f, waitingHere)}</span>
        <span>{feedLine}</span>
      </footer>}

      {toast && (
        <ToastView
          toast={toast}
          onHold={() => toastTimer.current && clearTimeout(toastTimer.current)}
          onRelease={() => toast.auto && showToast(toast, TOAST_AFTER_HOVER_MS)}
          onDismiss={dismissToast}
          onCancel={() => void cancelFollowUp()}
          onWatch={() => void watchFollowUp()}
        />
      )}

      {ui.dlg === "sessions" && (
        <SessionsDialog
          rows={rows}
          onClose={closeDlg}
          onNew={newSession}
          onOpen={(id) => {
            closeDlg();
            openPicked(id);
          }}
          copy={(t, w) => void copy(t, w)}
        />
      )}
      {ui.dlg === "allvars" && bar && <VarsDialog vars={bar.allVars} settings={bar.settings} onClose={closeDlg} />}
      {ui.dlg === "pkgs" && (
        <PackagesDialog
          key={viewedBlueprint ?? contextStatus.blueprint.name}
          blueprint={viewedBlueprint ?? contextStatus.blueprint.name}
          inForceLabel={versionInForce !== null ? "v" + versionInForce : ""}
          status={contextStatus}
          packages={packages}
          docs={data.packageDocs}
          docsErrors={snap.packageDocsErrors}
          onLoadDocs={store.loadPackageDocs}
          onClose={closeDlg}
          onOpenRules={(name, line) => patch({ dlg: "bp", bpV: null, bpFocus: { line, label: name + "’s own rules" } })}
        />
      )}
      {ui.dlg === "bp" && (
        <BlueprintView
          d={bpView}
          error={snap.versionErrors[bp.version] ?? ""}
          differs={projectHistoryVersion ? "This version is part of the project-wide blueprint history. The selected blueprint is " + contextStatus.blueprint.name + (versionInForce !== null ? " v" + versionInForce : "") + "." : versionInForce !== null && bp.version !== versionInForce ? notInForce(versionInForce) : ""}
          file={projectHistoryVersion ? "Project-wide blueprint history" : projectPath(contextStatus.blueprint.path, contextStatus.project)}
          onClose={closeDlg}
          onVersion={(v) => patch({ bpV: v, bpFocus: null })}
          onRetry={() => void store.loadVersion(bp.version)}
          copy={(t, w) => void copy(t, w)}
        />
      )}
      {ui.dlg === "confirm" && pending && confirm && (
        <ConfirmDialog
          title={confirm.title}
          body={confirm.body}
          go={confirm.go}
          command={commandOf(pending)}
          rerun={pending.kind === "rerun" ? {
            variables: pendingRerun && typeof pendingRerun !== "string" ? pendingRerun.variables : [],
            values: rerunVariables,
            onVariable: (name, value) => {
              setRerunVariables((old) => ({ ...old, [name]: value }));
              setRerunError("");
            },
            error: typeof pendingRerun === "string" ? pendingRerun : rerunError,
            running: rerunRunning,
          } : undefined}
          onGo={confirmGo}
          onClose={() => {
            setPending(null);
            closeDlg();
          }}
        />
      )}
      {ui.dlg === "draft" && draft && <DraftDialog draft={draft} refText={draftRef(f.names, draft)} onClose={closeDlg} copy={(t, w) => void copy(t, w)} />}
    </div>
  );
}

interface Failure {
  session: string | null;
  message: string;
  kind?: string;
  started: boolean;
  /** Stopped from the page rather than failed. */
  stopped?: boolean;
  /** Stopped by a limit the agent reached, as `AgentResult.stopped` names it. */
  reason?: string;
}

/** A message Claude Code refused until what it bills is confirmed, in the session it was sent in. */
interface BillingAsk {
  session: string | null;
  /** What the resend confirms, word for word. */
  confirmation: string;
  /** How the page names it. */
  billed: string;
  /** What a session confirmed has changed since. */
  changed: boolean;
  /** Cancel was pressed: the message stays unsent. */
  cancelled: boolean;
}

/** What a billing refusal asks to confirm, or `null` for any other outcome. */
function billingAsked(outcome: { kind: string; message: string }, status: Status | null): Omit<BillingAsk, "session"> | null {
  if (outcome.kind !== "billing-unconfirmed" && outcome.kind !== "billing-changed") return null;
  // The refusal names what to confirm as it is now; the status may be older.
  const cc = status?.claude_code;
  const confirmation = /--confirm-billing "([^"]*)"/.exec(outcome.message)?.[1] ?? cc?.confirmation ?? "";
  const billed = cc?.state === "available" && cc.confirmation === confirmation ? billedName(cc) : confirmation;
  return { confirmation, billed, changed: outcome.kind === "billing-changed", cancelled: false };
}

const FAILURE_KEY = "submilli-playground-chat-failure";
const FRESH_KEY = "submilli-playground-new-session";
const PANE_KEY = "submilli-playground-pane";
const LAYOUT_KEY = "submilli-playground-layout";
const viewKey = (session: string) => "submilli-playground-view:" + session;
const draftKey = (session: string | null) => "submilli-playground-draft:" + (session ?? "new");
function restoreView(session: string | null): UiState {
  const saved = session ? readKept<Partial<UiState>>(viewKey(session)) : null;
  return { ...INITIAL_UI, ...saved, mode: "open", playing: false, dlg: null, fresh: null };
}

/** Drafts are owned by composer identity, never by whichever mode happens to render next. */
function useComposerDraft(identity: string | null): [string, (value: string | ((text: string) => string)) => void] {
  const key = draftKey(identity);
  const currentKey = useRef(key);
  currentKey.current = key;
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const setText = useCallback((value: string | ((text: string) => string)) => {
    const owner = currentKey.current;
    setDrafts((saved) => {
      const previous = saved[owner] ?? readKept<string>(owner) ?? "";
      return { ...saved, [owner]: typeof value === "function" ? value(previous) : value };
    });
  }, []);
  useEffect(() => {
    for (const [owner, text] of Object.entries(drafts)) keep(owner, text);
  }, [drafts]);
  return [drafts[key] ?? readKept<string>(key) ?? "", setText];
}

/** What the tab kept under `key`, across reloads. */
function readKept<T>(key: string): T | null {
  try {
    const text = sessionStorage.getItem(key);
    return text ? (JSON.parse(text) as T) : null;
  } catch {
    return null;
  }
}

function keep(key: string, value: unknown) {
  try {
    if (value) sessionStorage.setItem(key, JSON.stringify(value));
    else sessionStorage.removeItem(key);
  } catch {
    // Storage is off; what it would keep lasts until the page reloads.
  }
}

/** The blueprint's file as the project names it: relative to the project's directory. */
function projectPath(path: string, project: string): string {
  const root = project.replace(/[\\/]+$/, "");
  return path.startsWith(root + "/") || path.startsWith(root + "\\") ? path.slice(root.length + 1) : path;
}
