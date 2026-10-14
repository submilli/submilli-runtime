// The page, once signed in: the header, the session bar, the summary, the step bar, the
// three panes, and the dialogs over them. One `UiState` says how the reader is looking at
// the record; the wording in `../model` turns it and the store's data into what shows.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { Draft, Show } from "../api/types";
import { sourceKey } from "../api/types";
import { packageRules, blueprintDialog } from "../model/blueprint";
import { askBox, chatEmptyText, chatItems, runWord, type ChatItem } from "../model/chat";
import { codePane } from "../model/code";
import { capitalize, copyNotice, end, plainBy } from "../model/format";
import { expandAll, logPane, type Action, type RuleSpot } from "../model/log";
import { actionBlock, footer, sessionBar, sessionRows } from "../model/sessions";
import { checkSetup, initialSetup, INITIAL_SETUP, type SetupForm } from "../model/setup";
import { AT_START, frame, nextKey, playFrom, prevKey, runsInFlight } from "../model/steps";
import { summary } from "../model/summary";
import { INITIAL_UI, type Dialog, type UiState } from "../model/ui";
import type { NewSessionSettings, PageStore, Snapshot } from "../store/store";
import { ChatPane } from "./ChatPane";
import { CodePane } from "./CodePane";
import { BlueprintView, Callout, ConfirmDialog, DraftDialog, MorePanel, PackagesDialog, SessionsDialog, ToastView, VarsDialog, type Toast } from "./Dialogs";
import { Header } from "./Header";
import { LogPane, type LogActions } from "./LogPane";
import { SessionBar } from "./SessionBar";
import { StepBar, Summary } from "./Summary";
import { useFollow } from "./useFollow";
import { UNREACHABLE } from "../api/client";
import { runHash } from "../api/login";
import {
  newSessionNotice,
  NO_SESSIONS_CHAT,
  RECONNECTING,
  SESSION_LOADING,
  SOURCE_LOADING,
  WAITING,
  answering,
  blueprintRefused,
  chatFailed,
  commandOf,
  confirmOf,
  draftRef,
  feedStopped,
  followUpDone,
  gapNote,
  notInForce,
  runMissing,
} from "./wording";

// How long a toast stays up, and how long after the pointer leaves it.
const TOAST_MS = 5000;
const TOAST_AFTER_HOVER_MS = 2000;
const PLAY_MS = 900;

/** Whether keys go to text the reader can edit; a read-only ask box still lets arrows step. */
function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const field = target.closest("input, textarea, select, [contenteditable='true']");
  return !!field && !((field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement) && field.readOnly);
}

export function Page({ store, snap }: { store: PageStore; snap: Snapshot }) {
  const { data } = snap;
  const status = data.status;
  // A session started but not yet sent its first message lives only in the page, so the
  // tab keeps it: a reload comes back to it rather than to the latest session.
  const [ui, setUi] = useState<UiState>(() => {
    const fresh = readKept<UiState["fresh"]>(FRESH_KEY);
    return fresh ? { ...INITIAL_UI, mode: "fresh", fresh } : INITIAL_UI;
  });
  const patch = useCallback((p: Partial<UiState> | ((u: UiState) => Partial<UiState>)) => setUi((u) => ({ ...u, ...(typeof p === "function" ? p(u) : p) })), []);
  const [form, setForm] = useState<SetupForm>(INITIAL_SETUP);
  // The view New session was pressed from, so Back can return to it.
  const [setupFrom, setSetupFrom] = useState<UiState | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const [pending, setPending] = useState<Action | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [chatText, setChatText] = useState("");
  const [away, setAway] = useState({ chat: false, log: false });
  // A message the agent couldn't answer, shown where it was sent: in a session, or in the new
  // one (`null`). Kept for the tab, so a reload still says why the message has no answer.
  const [failure, setFailureState] = useState<Failure | null>(() => readKept<Failure>(FAILURE_KEY));
  const setFailure = (f: Failure | null) => {
    setFailureState(f);
    keep(FAILURE_KEY, f);
  };
  useEffect(() => {
    keep(FRESH_KEY, ui.mode === "fresh" && ui.fresh && ui.fresh.messages.length === 0 ? ui.fresh : null);
  }, [ui.mode, ui.fresh]);
  // The run Cancel was pressed for, until the playground confirms.
  const [cancelling, setCancelling] = useState<number | null>(null);
  // An action the page sent and is waiting on, so a second click doesn't send it again.
  const busy = useRef(false);
  const askInput = useRef<HTMLInputElement>(null);
  // A row picked before it exists (a new rerun) comes into view once it does.
  const scrollTo = useRef<string | null>(null);
  // The new row of a test or rerun the page started, to take focus once it shows.
  const focusRow = useRef<string | null>(null);

  const f = useMemo(() => frame(data, ui), [data, ui]);
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
  const moreButton = useRef<HTMLButtonElement>(null);
  const openDlg = (dlg: Dialog, extra: Partial<UiState> = {}) => {
    opener.current = document.activeElement;
    patch({ dlg, more: false, ...extra });
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
  useEffect(() => {
    if (shown.current === data.sessionId) return;
    shown.current = data.sessionId;
    setChatText("");
    // A dialog or More the reader opened meanwhile stays open.
    setUi((u) => (u.mode === "setup" ? u : { ...INITIAL_UI, focus: u.focus, dlg: u.dlg, more: u.more }));
    setSetupFrom(ui.mode === "setup" && data.sessionId !== null ? INITIAL_UI : null);
    // A run's link names one session; once another shows, it no longer applies. One still
    // waiting for its session to show stays.
    if (!snap.requested || requestedDone.current === snap.requested.run) forgetRunLink();
  }, [data.sessionId]);

  // A run the link asked for is picked and opened once its session shows and its record is loaded.
  const requestedDone = useRef<number | null>(null);
  // The row the link picked; once the reader picks another, the link no longer says what's shown.
  const linkSel = useRef<string | null>(null);
  useEffect(() => {
    if (linkSel.current !== null && ui.sel !== null && ui.sel !== linkSel.current) {
      linkSel.current = null;
      forgetRunLink();
    }
  }, [ui.sel]);
  // The address stops naming a run, so a reload comes back to what's shown instead.
  const forgetRunLink = () => {
    if (snap.requested) store.clearRequested();
    if (location.hash.startsWith("#run=")) history.replaceState(null, "", location.pathname + location.search);
  };
  useEffect(() => {
    const req = snap.requested;
    if (!req || requestedDone.current === req.run || snap.sessionLoading) return;
    const show = data.runs[req.run];
    if (!show) return;
    const source = show.test?.source_run ?? show.rerun_of ?? null;
    const home = source !== null ? data.runs[source]?.session : show.session;
    if (!home || home !== data.sessionId) return;
    requestedDone.current = req.run;
    // The run the link names shows whatever the log was filtered to.
    patch({ filter: "all" });
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
  );
  // New runs below are something to follow; a rerun opening under a run above isn't.
  const logFollow = useFollow<HTMLDivElement>(
    f.vis.length + f.running.length,
    useCallback((a: boolean) => setAway((w) => ({ ...w, log: a })), []),
    data.sessionId,
  );
  const toLatest = () => {
    patch({ cursor: null, playing: false });
    chatFollow.toEnd();
    logFollow.toEnd();
  };

  useEffect(() => {
    if (!ui.playing) return;
    const timer = setInterval(() => {
      setUi((u) => {
        const n = nextKey(f.steps, u.cursor);
        return n === null ? { ...u, cursor: null, playing: false } : { ...u, cursor: n };
      });
    }, PLAY_MS);
    return () => clearInterval(timer);
  }, [ui.playing, f.steps]);
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

  const act = async (action: Action, confirmed = false) => {
    if (blocked || busy.current) return;
    if (!confirmed && confirmOf(action)) {
      setPending(action);
      openDlg("confirm");
      return;
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
      showToast({ head: "Running:", body: commandOf(action), auto: false });
      const outcome = action.kind === "test" ? await store.test(action.run, action.mode) : await store.rerun(action.run);
      if (!outcome.ok) return refused(outcome.message);
      const made: Show = outcome.result;
      const source = action.run;
      // The new run shows under the run it followed, opened so it can be seen.
      focusRow.current = "f" + made.run;
      patch((u) => ({
        sel: "f" + made.run,
        open: { ...u.open, ["r" + source]: true, ["f" + made.run]: true },
        followOpen: { ...u.followOpen, [source]: true },
      }));
      showToast({ head: followUpDone(source, made.run, end(made.outcome)), auto: true });
    } finally {
      busy.current = false;
    }
  };

  const confirmGo = () => {
    const action = pending;
    setPending(null);
    closeDlg();
    if (action) void act(action, true);
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
    showToast({ head: "Cancelling run " + run + ": it ends as cancelled once its calls drain.", auto: true });
  };
  useEffect(() => {
    if (cancelling === null || runsInFlight(data.events).includes(cancelling)) return;
    const run = cancelling;
    setCancelling(null);
    // The notice that it's cancelling gives way to how it ended.
    if (toast && toast.head.startsWith("Cancelling run " + run + ":")) showToast({ head: "Run " + run + " ended: " + runWord(f, run) + ".", auto: true });
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
    // The form is in the session bar, which focus view hides.
    setUi(() => ({ ...INITIAL_UI, mode: "setup" }));
    setForm(initialSetup(status));
    // A message typed for another session isn't the new one's first.
    setChatText("");
  };
  const goBack = () => {
    if (setupFrom) setUi({ ...setupFrom, focus: false, dlg: null, more: false });
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
      fresh: { variables: check.variables, agent: check.model, settings: check.settings, started: new Date().toISOString(), messages: [] },
    });
    // The next thing to do is say what the agent should do.
    setTimeout(() => askInput.current?.focus(), 0);
  };

  const send = async () => {
    const text = chatText.trim();
    // One message at a time: a second while the first is unanswered would start another chat.
    if (!text || snap.pendingChat) return;
    setChatText("");
    setFailure(null);
    const fresh = ui.mode === "fresh" ? ui.fresh : null;
    if (fresh) {
      patch({ fresh: { ...fresh, messages: [...fresh.messages, text] } });
      const settings: NewSessionSettings = {
        model: fresh.agent,
        variables: fresh.variables,
        effort: fresh.settings?.effort,
        max_output: fresh.settings?.max_output,
        provider_settings: fresh.settings?.provider,
      };
      const { outcome, session } = await store.chat(text, settings);
      if (!outcome.ok) {
        // The failure shows where the message went: in the session the playground made
        // for it, or, when none was made, in the new session still being set up.
        setFailure({ session, message: outcome.message, started: session !== null });
        if (session === null) setUi((u) => (u.mode === "fresh" && u.fresh ? { ...u, fresh: { ...u.fresh, messages: u.fresh.messages.filter((m) => m !== text) } } : u));
        setChatText(text);
        return;
      }
      // The answer can come before the session is listed; it is shown now either way.
      setUi((u) => (u.mode === "fresh" ? { ...INITIAL_UI, focus: u.focus, dlg: u.dlg, more: u.more } : u));
      return;
    }
    const { outcome, session } = await store.chat(text);
    if (!outcome.ok) {
      setFailure({ session, message: outcome.message, started: true });
      // Kept for the reader to send again, in the session it was written for.
      if (session === data.sessionId) setChatText(text);
    }
  };

  // ---- the keyboard

  const onKey = (ev: KeyboardEvent) => {
    if (ev.key === "Escape") {
      if (ui.dlg) {
        closeDlg();
        setPending(null);
      } else if (ui.more) {
        patch({ more: false });
        moreButton.current?.focus();
      } else if (toast) dismissToast();
      else if (ui.focus) leaveFocus();
      return;
    }
    if (ev.key !== "ArrowLeft" && ev.key !== "ArrowRight") return;
    if (ui.mode !== "open" || ui.dlg || ev.altKey || ev.ctrlKey || ev.metaKey || typing(ev.target)) return;
    ev.preventDefault();
    if (ev.key === "ArrowLeft" && !f.bar.atStart) prev();
    if (ev.key === "ArrowRight" && !f.bar.atEnd) next();
  };
  // The notice's button goes with it, so focus goes on to the ask box.
  const dismissNotice = () => {
    store.dismissNotice();
    setTimeout(() => askInput.current?.focus(), 0);
  };
  const dismissToast = () => {
    setToast(null);
    const back = toastFrom.current;
    toastFrom.current = null;
    if (back instanceof HTMLElement && back.isConnected && back !== document.body) back.focus();
  };
  // Focus view's own button goes when it ends, so focus moves to More, which turns it on again.
  const leaveFocus = () => {
    patch({ focus: false, more: false });
    setTimeout(() => moreButton.current?.focus(), 0);
  };
  // Turning it on from More, whose panel closes, puts focus on the button that turns it off.
  const toggleFocus = () => {
    if (ui.focus) return leaveFocus();
    patch({ focus: true, more: false });
    setTimeout(() => document.querySelector<HTMLElement>(".pg-exit-focus")?.focus(), 0);
  };
  const keyRef = useRef(onKey);
  keyRef.current = onKey;
  useEffect(() => {
    const listener = (ev: KeyboardEvent) => keyRef.current(ev);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
  // A click outside More closes it, as Escape does; its own button toggles it.
  useEffect(() => {
    if (!ui.more) return;
    const listener = (ev: PointerEvent) => {
      const target = ev.target instanceof Node ? ev.target : null;
      if (target && (document.querySelector(".pg-more")?.contains(target) || moreButton.current?.contains(target))) return;
      patch({ more: false });
    };
    document.addEventListener("pointerdown", listener);
    return () => document.removeEventListener("pointerdown", listener);
  }, [ui.more, patch]);

  if (!status) return null;

  // ---- what shows

  const bar = sessionBar(f);
  const chat: ChatItem[] = chatItems(f);
  // A message sent in the shown session waits here until the feed brings the answer.
  const waitingHere = !!snap.pendingChat && ui.mode === "open" && snap.pendingChat.session === data.sessionId;
  if (waitingHere && !f.stepping) chat.push({ kind: "wait", text: WAITING });
  const failedHere = failure && (ui.mode === "fresh" ? failure.session === null : ui.mode === "open" && failure.session === data.sessionId);
  if (failure && failedHere && !f.stepping) chat.push({ kind: "failed", ...chatFailed(failure.message, failure.started) });
  const box = askBox(f);
  const askState = snap.unreachable
    ? { ...box, disabled: true, primary: false, reason: UNREACHABLE.split(". ")[0] + "." }
    : snap.pendingChat && !box.disabled
      ? { ...box, disabled: true, primary: false, reason: answering(ui.mode === "open" && snap.pendingChat.session === data.sessionId ? null : snap.pendingChat.session ?? "new") }
      : box;
  const pane = codePane(f);
  const srcKey = atPackage ? sourceKey(atPackage.package, atPackage.module) : null;
  const srcError = srcKey ? snap.sourceErrors[srcKey] : undefined;
  const srcLoading = srcKey && !data.sources[srcKey] && !srcError ? SOURCE_LOADING : "";
  const log = logPane(f);
  const gaps = ui.mode === "open" ? data.events.flatMap((e) => ("gap" in e.body ? [gapNote(e.run, e.body.gap)] : [])) : [];
  const rows = sessionRows(f);
  const refusedEdit = status.blueprint_status.refused;
  const bp = blueprintDialog(f);
  const versionInForce = f.inForce;

  const logActions: LogActions = {
    blocked,
    act: (a) => void act(a),
    copy: (t, w) => void copy(t, w),
    toggle: (id) => patch((u) => ({ open: { ...u.open, [id]: !u.open[id] }, sel: id, program: null })),
    toggleChecks: (run, open) => patch((u) => ({ checksOpen: { ...u.checksOpen, [run]: open } })),
    toggleFollow: (run, open) => patch((u) => ({ followOpen: { ...u.followOpen, [run]: open } })),
    showLine: (id, run) => patch({ sel: id, program: run }),
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
  };

  // The commands the More panel shows use this session's own numbers: a program that
  // compiled, and a check, preferably a refused one.
  const endedShows = f.ended.flatMap((n) => (data.runs[n] ? [data.runs[n]] : []));
  const firstRun = (endedShows.find((r) => end(r.outcome) !== "rejected") ?? endedShows[0])?.run ?? null;
  const checks = endedShows.flatMap((r) => r.decisions);
  const firstCheck = (checks.find((d) => d.outcome === "deny") ?? checks[0])?.refs[0];
  const moreCommands = [
    firstRun !== null ? "submilli playground show " + firstRun + " --program" : "",
    firstCheck ? "submilli playground explain " + firstCheck : "",
    versionInForce !== null ? "submilli playground changes --version " + versionInForce : "",
  ].filter(Boolean);

  const inForceText = versionInForce !== null ? data.versions[versionInForce]?.text : undefined;
  const packages = status.packages.map((p) => ({ name: p.name, rules: inForceText ? packageRules(inForceText, p.name) : null }));

  const feedLine = snap.feed === "reconnecting" && !snap.unreachable ? RECONNECTING : "";
  // A rerun gets only the bound variables its blueprint declares; the server drops the rest.
  const declared = new Set(status.declared_variables.map((d) => d.name));
  const rerunVars = Object.fromEntries(Object.entries(status.binding.variables).filter(([k]) => declared.has(k)));
  const confirm = pending ? confirmOf(pending, rerunVars) : null;

  return (
    <div className={"pg" + (ui.focus ? " focus" : "")}>
      <Header
        names={f.names}
        status={status}
        inForce={versionInForce}
        focus={ui.focus}
        setup={ui.mode === "setup"}
        moreOpen={ui.more}
        moreRef={moreButton}
        onBlueprint={() => openDlg("bp", { bpV: null, bpFocus: null })}
        onPackages={() => openDlg("pkgs")}
        onFocus={toggleFocus}
        onSessions={() => {
          // The list is read again as it opens, so a session started a moment ago is in it.
          void store.refreshSessions();
          openDlg("sessions");
        }}
        onNewSession={newSession}
        onMore={() => patch({ more: !ui.more })}
      />

      {!ui.focus && (
        <SessionBar
          bar={bar && (snap.unreachable || snap.feed === "reconnecting" || snap.feedError) ? { ...bar, live: false } : bar}
          setup={ui.mode === "setup"}
          noSessions={snap.noSessions}
          status={status}
          form={form}
          canGoBack={ui.mode === "setup" && !!setupFrom}
          cancelRun={cancelRun}
          cancelling={cancelling !== null && cancelling === cancelRun}
          onForm={setForm}
          onStart={startSession}
          onBack={goBack}
          onCopy={() =>
            bar && bar.copyText
              ? void copy(bar.copyText, "reference")
              : showToast({ head: "No reference yet", body: "This session starts on the playground when you send its first message.", auto: true })
          }
          onAllVars={() => openDlg("allvars")}
          onCancel={(run) => void cancel(run)}
          copy={(t, w) => void copy(t, w)}
        />
      )}

      {snap.unreachable && <Callout text={UNREACHABLE} tone="bad" />}
      {refusedEdit && <Callout text={blueprintRefused(f.names.blueprint, refusedEdit, versionInForce)} tone="bad" />}
      {snap.feedError && <Callout text={feedStopped()} tone="bad" />}
      {snap.requestedMissing !== null && (
        <Callout text={runMissing(snap.requestedMissing)}>
          <button className="pg-link" onClick={dismissNotice}>
            Dismiss
          </button>
        </Callout>
      )}
      {snap.newSession && (
        <Callout text={newSessionNotice(snap.newSession, data.sessions.find((s) => s.session === snap.newSession)?.untrusted?.task)}>
          <button className="pg-link" onClick={() => snap.newSession && void store.openSession(snap.newSession)}>
            Open it
          </button>
          <button className="pg-link" onClick={dismissNotice}>
            Dismiss
          </button>
        </Callout>
      )}

      {!ui.focus && <Summary s={summary(f)} />}

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

      <main className="pg-app">
        <ChatPane
          items={chat}
          empty={snap.noSessions && ui.mode === "open" ? NO_SESSIONS_CHAT : ui.mode === "open" && (snap.sessionLoading || snap.feed === "connecting") ? SESSION_LOADING : chatEmptyText(f)}
          box={snap.noSessions && ui.mode === "open" ? { placeholder: "Start a session to chat", disabled: true, primary: false } : askState}
          text={chatText}
          scrollRef={chatFollow.ref}
          inputRef={askInput}
          onScroll={chatFollow.onScroll}
          onText={setChatText}
          onSend={() => void send()}
          onPick={(run) => patch((u) => ({ sel: "r" + run, program: null, open: { ...u.open, ["r" + run]: true } }))}
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
      </main>

      <footer className="pg-foot">
        <span>{failedHere && !f.stepping && !waitingHere ? "Live session · the agent couldn’t answer the last message" : snap.feedError ? "Live updates stopped" : footer(f, waitingHere)}</span>
        <span>{feedLine}</span>
      </footer>

      {toast && (
        <ToastView
          toast={toast}
          onHold={() => toastTimer.current && clearTimeout(toastTimer.current)}
          onRelease={() => toast.auto && showToast(toast, TOAST_AFTER_HOVER_MS)}
          onDismiss={dismissToast}
        />
      )}

      {ui.more && <MorePanel commands={moreCommands} focus={ui.focus} onFocus={toggleFocus} onLeave={(to) => !moreButton.current?.contains(to) && patch({ more: false })}
          onBack={() => {
            patch({ more: false });
            moreButton.current?.focus();
          }}
        />}

      {ui.dlg === "sessions" && (
        <SessionsDialog
          rows={rows}
          onClose={closeDlg}
          onNew={newSession}
          onOpen={(id) => {
            closeDlg();
            if (id === data.sessionId) setUi((u) => ({ ...INITIAL_UI, focus: u.focus }));
            else void store.openSession(id);
          }}
          copy={(t, w) => void copy(t, w)}
        />
      )}
      {ui.dlg === "allvars" && bar && <VarsDialog vars={bar.allVars} settings={bar.settings} onClose={closeDlg} />}
      {ui.dlg === "pkgs" && (
        <PackagesDialog
          blueprint={f.names.blueprint}
          inForceLabel={versionInForce !== null ? "v" + versionInForce : ""}
          status={status}
          packages={packages}
          onClose={closeDlg}
          onOpenRules={(name, line) => patch({ dlg: "bp", bpV: null, bpFocus: { line, label: name + "’s own rules" } })}
        />
      )}
      {ui.dlg === "bp" && (
        <BlueprintView
          d={bp}
          differs={versionInForce !== null && bp.version !== versionInForce ? notInForce(versionInForce) : ""}
          file={projectPath(status.blueprint.path, status.project)}
          onClose={closeDlg}
          onVersion={(v) => patch({ bpV: v, bpFocus: null })}
          copy={(t, w) => void copy(t, w)}
        />
      )}
      {ui.dlg === "confirm" && pending && confirm && (
        <ConfirmDialog
          title={confirm.title}
          body={confirm.body}
          go={confirm.go}
          command={commandOf(pending)}
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
  started: boolean;
}

const FAILURE_KEY = "submilli-playground-chat-failure";
const FRESH_KEY = "submilli-playground-new-session";

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
