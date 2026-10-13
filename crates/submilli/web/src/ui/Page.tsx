// The page, once signed in: the header, the session bar, the summary, the step bar, the
// three panes, and the dialogs over them. One `UiState` says how the reader is looking at
// the record; the wording in `../model` turns it and the store's data into what shows.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { Draft, Show } from "../api/types";
import { sourceKey } from "../api/types";
import { packageRules, blueprintDialog } from "../model/blueprint";
import { askBox, chatEmptyText, chatItems, type ChatItem } from "../model/chat";
import { codePane } from "../model/code";
import { copyNotice, end, plainBy } from "../model/format";
import { expandAll, logPane, type Action, type RuleSpot } from "../model/log";
import { actionBlock, footer, sessionBar, sessionRows } from "../model/sessions";
import { checkSetup, initialSetup, INITIAL_SETUP, type SetupForm } from "../model/setup";
import { AT_START, frame, nextKey, playFrom, prevKey, runsInFlight } from "../model/steps";
import { summary } from "../model/summary";
import { INITIAL_UI, type Dialog, type UiState } from "../model/ui";
import type { NewSessionSettings, PageStore, Snapshot } from "../store/store";
import { ChatPane } from "./ChatPane";
import { CodePane } from "./CodePane";
import { BlueprintView, ConfirmDialog, DraftDialog, MorePanel, PackagesDialog, SessionsDialog, ToastView, VarsDialog, type Toast } from "./Dialogs";
import { Header } from "./Header";
import { LogPane, type LogActions } from "./LogPane";
import { SessionBar } from "./SessionBar";
import { StepBar, Summary } from "./Summary";
import { useFollow } from "./useFollow";
import {
  NEW_SESSION_NOTICE,
  RECONNECTING,
  SOURCE_LOADING,
  WAITING,
  commandOf,
  confirmOf,
  draftRef,
  feedStopped,
  followUpDone,
  gapNote,
  notInForce,
} from "./wording";

// How long a toast stays up, and how long after the pointer leaves it.
const TOAST_MS = 5000;
const TOAST_AFTER_HOVER_MS = 2000;
const PLAY_MS = 900;

function typing(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && !!target.closest("input, textarea, select, [contenteditable='true']");
}

export function Page({ store, snap }: { store: PageStore; snap: Snapshot }) {
  const { data } = snap;
  const status = data.status;
  const [ui, setUi] = useState<UiState>(INITIAL_UI);
  const patch = useCallback((p: Partial<UiState> | ((u: UiState) => Partial<UiState>)) => setUi((u) => ({ ...u, ...(typeof p === "function" ? p(u) : p) })), []);
  const [form, setForm] = useState<SetupForm>(INITIAL_SETUP);
  // The view New session was pressed from, so Back can return to it.
  const [setupFrom, setSetupFrom] = useState<UiState | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const [pending, setPending] = useState<Action | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [chatText, setChatText] = useState("");
  const [away, setAway] = useState({ chat: false, log: false });

  const f = useMemo(() => frame(data, ui), [data, ui]);
  const blocked = actionBlock(f);

  // ---- toasts and copying

  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const showToast = useCallback((t: Toast, ms = TOAST_MS) => {
    if (toastTimer.current) clearTimeout(toastTimer.current);
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

  // Another session shown starts the reader over, at its latest event.
  const shown = useRef(data.sessionId);
  useEffect(() => {
    if (shown.current === data.sessionId) return;
    shown.current = data.sessionId;
    setUi((u) => ({ ...INITIAL_UI, focus: u.focus }));
    setSetupFrom(null);
  }, [data.sessionId]);

  // A run the link asked for is picked and opened once its record is loaded.
  const requestedDone = useRef<number | null>(null);
  useEffect(() => {
    const req = snap.requested;
    if (!req || requestedDone.current === req.run) return;
    const show = data.runs[req.run];
    if (!show) return;
    requestedDone.current = req.run;
    const source = show.test?.source_run ?? show.rerun_of ?? null;
    if (source !== null) {
      patch((u) => ({ sel: "f" + req.run, open: { ...u.open, ["r" + source]: true, ["f" + req.run]: true }, followOpen: { ...u.followOpen, [source]: true } }));
    } else if (req.decision !== null) {
      const ref = req.run + "." + req.decision;
      patch((u) => ({ sel: "d" + ref, open: { ...u.open, ["r" + req.run]: true, ["d" + ref]: true }, checksOpen: { ...u.checksOpen, [req.run]: true } }));
    } else {
      patch((u) => ({ sel: "r" + req.run, open: { ...u.open, ["r" + req.run]: true } }));
    }
  }, [snap.requested, data.runs, patch]);

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

  // A picked row comes into view.
  useEffect(() => {
    if (ui.sel) document.getElementById("pgrow-" + ui.sel)?.scrollIntoView?.({ block: "nearest" });
  }, [ui.sel]);

  // ---- stepping

  const steps = f.steps;
  const go = (cursor: string | null) => patch({ cursor, playing: false, sel: null, program: null });
  const prev = () => go(prevKey(steps, ui.cursor));
  const next = () => go(nextKey(steps, ui.cursor));
  const chatFollow = useFollow<HTMLDivElement>(
    f.vis.length + (snap.pendingChat ? 1 : 0),
    useCallback((a: boolean) => setAway((w) => ({ ...w, chat: a })), []),
  );
  const logFollow = useFollow<HTMLDivElement>(
    f.vis.length + Object.keys(data.runs).length,
    useCallback((a: boolean) => setAway((w) => ({ ...w, log: a })), []),
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

  const refused = (message: string) => showToast({ head: message, auto: false });

  const act = async (action: Action, confirmed = false) => {
    if (blocked) return;
    if (!confirmed && confirmOf(action)) {
      setPending(action);
      openDlg("confirm");
      return;
    }
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
    patch((u) => ({
      sel: "f" + made.run,
      open: { ...u.open, ["r" + source]: true },
      followOpen: { ...u.followOpen, [source]: true },
    }));
    showToast({ head: followUpDone(source, made.run, end(made.outcome)), auto: true });
  };

  const confirmGo = () => {
    const action = pending;
    setPending(null);
    closeDlg();
    if (action) void act(action, true);
  };

  const cancel = async (run: number) => {
    const outcome = await store.cancel(run);
    if (!outcome.ok) return refused(outcome.message);
    showToast({ head: "Cancelled run " + run + ".", auto: true });
  };

  // ---- chat and new sessions

  const newSession = () => {
    if (!status) return;
    setSetupFrom(ui.mode === "setup" ? setupFrom : ui);
    setUi((u) => ({ ...INITIAL_UI, mode: "setup", focus: u.focus }));
    setForm(initialSetup(status));
  };
  const goBack = () => {
    if (setupFrom) setUi(setupFrom);
    setSetupFrom(null);
  };
  const startSession = () => {
    if (!status) return;
    const check = checkSetup(status, form);
    if (check.cantStart) return;
    setSetupFrom(null);
    patch({
      mode: "fresh",
      fresh: { variables: check.variables, agent: check.model, settings: check.settings, started: new Date().toISOString(), messages: [] },
    });
  };

  const send = async () => {
    const text = chatText.trim();
    // One message at a time: a second while the first is unanswered would start another chat.
    if (!text || snap.pendingChat) return;
    setChatText("");
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
      const outcome = await store.chat(text, settings);
      if (!outcome.ok) {
        patch((u) => (u.mode === "fresh" && u.fresh ? { fresh: { ...u.fresh, messages: u.fresh.messages.filter((m) => m !== text) } } : {}));
        setChatText(text);
        return refused(outcome.message);
      }
      // The answer can come before the session is listed; it is shown now either way.
      setUi((u) => (u.mode === "fresh" ? { ...INITIAL_UI, focus: u.focus } : u));
      return;
    }
    const outcome = await store.chat(text);
    if (!outcome.ok) {
      setChatText(text);
      refused(outcome.message);
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
      } else if (toast) setToast(null);
      else if (ui.focus) patch({ focus: false });
      return;
    }
    if (ev.key !== "ArrowLeft" && ev.key !== "ArrowRight") return;
    if (ui.mode !== "open" || ui.dlg || ev.altKey || ev.ctrlKey || ev.metaKey || typing(ev.target)) return;
    ev.preventDefault();
    if (ev.key === "ArrowLeft" && !f.bar.atStart) prev();
    if (ev.key === "ArrowRight" && !f.bar.atEnd) next();
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
  // A reply sent to the shown session waits here until the feed brings the answer.
  if (snap.pendingChat && ui.mode === "open" && !f.stepping) chat.push({ kind: "wait", text: WAITING });
  const box = askBox(f);
  const askState = snap.pendingChat ? { ...box, disabled: true, primary: false } : box;
  const pane = codePane(f);
  const srcKey = atPackage ? sourceKey(atPackage.package, atPackage.module) : null;
  const srcError = srcKey ? snap.sourceErrors[srcKey] : undefined;
  const srcLoading = srcKey && !data.sources[srcKey] && !srcError ? SOURCE_LOADING : "";
  const log = logPane(f);
  const gaps = ui.mode === "open" ? data.events.flatMap((e) => ("gap" in e.body ? [gapNote(e.run, e.body.gap)] : [])) : [];
  const rows = sessionRows(f);
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
    openTried: (run) => patch((u) => ({ open: { ...u.open, ["f" + run]: true }, sel: "f" + run })),
    inspect: (spot: RuleSpot) => openDlg("bp", { bpV: spot.version, bpFocus: { line: spot.line, label: spot.label } }),
    cancel: (run) => void cancel(run),
  };

  // The commands the More panel shows use this session's own numbers.
  const firstRun = f.ended[0] ?? null;
  const firstCheck = firstRun !== null ? data.runs[firstRun]?.decisions[0]?.refs[0] : undefined;
  const moreCommands = [
    firstRun !== null ? "submilli playground show " + firstRun + " --program" : "",
    firstCheck ? "submilli playground explain " + firstCheck : "",
    versionInForce !== null ? "submilli playground changes --version " + versionInForce : "",
  ].filter(Boolean);

  const inForceText = versionInForce !== null ? data.versions[versionInForce]?.text : undefined;
  const packages = status.packages.map((p) => ({ name: p.name, rules: inForceText ? packageRules(inForceText, p.name) : null }));

  const feedLine = snap.feed === "reconnecting" ? RECONNECTING : snap.feedError ? feedStopped(snap.feedError) : "";
  const confirm = pending ? confirmOf(pending) : null;

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
        onFocus={() => patch({ focus: !ui.focus, more: false })}
        onSessions={() => openDlg("sessions")}
        onNewSession={newSession}
        onMore={() => patch({ more: !ui.more })}
      />

      {!ui.focus && (
        <SessionBar
          bar={bar}
          setup={ui.mode === "setup"}
          noSessions={snap.noSessions}
          status={status}
          form={form}
          canGoBack={ui.mode === "setup" && !!setupFrom}
          cancelRun={cancelRun}
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

      {snap.newSession && (
        <div className="pg-callout" role="note">
          {NEW_SESSION_NOTICE}{" "}
          <button className="pg-link" onClick={() => snap.newSession && store.openSession(snap.newSession)}>
            Open it
          </button>
          <button className="pg-link" onClick={store.dismissNotice}>
            Dismiss
          </button>
        </div>
      )}

      {!ui.focus && <Summary s={summary(f)} />}

      {ui.mode === "open" && (
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
          empty={snap.noSessions && ui.mode === "open" ? "" : chatEmptyText(f)}
          box={askState}
          text={chatText}
          scrollRef={chatFollow.ref}
          onScroll={chatFollow.onScroll}
          onText={setChatText}
          onSend={() => void send()}
          onPick={(run) => patch({ sel: "r" + run, program: null })}
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
          cancelRun={f.atCall && f.atCall.run === cancelRun ? cancelRun : null}
          scrollRef={logFollow.ref}
          onScroll={logFollow.onScroll}
          onFilter={(filter) => patch({ filter })}
          onToggleAll={() => patch(expandAll(log, ui))}
          a={logActions}
        />
      </main>

      <footer className="pg-foot">
        <span>{footer(f)}</span>
        <span>{feedLine}</span>
      </footer>

      {toast && (
        <ToastView
          toast={toast}
          onHold={() => toastTimer.current && clearTimeout(toastTimer.current)}
          onRelease={() => toast.auto && showToast(toast, TOAST_AFTER_HOVER_MS)}
          onDismiss={() => setToast(null)}
        />
      )}

      {ui.more && <MorePanel commands={moreCommands} focus={ui.focus} onFocus={() => patch({ focus: !ui.focus, more: false })} />}

      {ui.dlg === "sessions" && (
        <SessionsDialog
          rows={rows}
          onClose={closeDlg}
          onNew={newSession}
          onOpen={(id) => {
            closeDlg();
            if (id === data.sessionId) setUi((u) => ({ ...INITIAL_UI, focus: u.focus }));
            else store.openSession(id);
          }}
          copy={(t, w) => void copy(t, w)}
        />
      )}
      {ui.dlg === "allvars" && bar && <VarsDialog vars={bar.allVars} onClose={closeDlg} />}
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

/** The blueprint's file as the project names it: relative to the project's directory. */
function projectPath(path: string, project: string): string {
  const root = project.replace(/[\\/]+$/, "");
  return path.startsWith(root + "/") || path.startsWith(root + "\\") ? path.slice(root.length + 1) : path;
}
