// The page's dialogs, the More panel and the toast. Each dialog takes focus when it opens;
// the page gives focus back to whatever opened it when it closes.
import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";

import type { Draft, Status } from "../api/types";
import type { BlueprintDialog } from "../model/blueprint";
import { forAsst } from "../model/refs";
import type { FixedVar, SessionRow } from "../model/sessions";
import { CloseIcon, cls, Inline } from "./controls";
import { BP_LOADING, DRAFT_NOTE, draftCommand, draftWhere } from "./wording";

function useTakeFocus<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el || el.contains(document.activeElement)) return;
    const first = el.querySelector<HTMLElement>("[data-autofocus]");
    (first ?? el).focus();
  }, []);
  return ref;
}

const FOCUSABLE = "button, [href], input, textarea, select, [tabindex]:not([tabindex='-1'])";

/** Tab and Shift+Tab go round the dialog's own controls, not out to the page behind it. */
function keepFocusIn(ev: KeyboardEvent<HTMLDivElement>) {
  if (ev.key !== "Tab") return;
  const all = [...ev.currentTarget.querySelectorAll<HTMLElement>(FOCUSABLE)];
  if (!all.length) return;
  const first = all[0];
  const last = all[all.length - 1];
  const at = document.activeElement;
  if (ev.shiftKey && (at === first || at === ev.currentTarget)) {
    ev.preventDefault();
    last.focus();
  } else if (!ev.shiftKey && at === last) {
    ev.preventDefault();
    first.focus();
  }
}

function Dialog(props: { id: string; title: ReactNode; size: "wide" | "narrow" | "bp"; onClose: () => void; actions?: ReactNode; children: ReactNode }) {
  const ref = useTakeFocus<HTMLDivElement>();
  // Only a click pressed and let go on the overlay closes it; one that starts or ends in
  // the dialog, such as selecting text past its edge, doesn't.
  const pressedOn = useRef<EventTarget | null>(null);
  const releasedOn = useRef<EventTarget | null>(null);
  return (
    <div
      className="pg-overlay"
      onPointerDown={(ev) => (pressedOn.current = ev.target)}
      onPointerUp={(ev) => (releasedOn.current = ev.target)}
      onClick={(ev) => ev.target === ev.currentTarget && pressedOn.current === ev.currentTarget && releasedOn.current === ev.currentTarget && props.onClose()}
    >
      <div className={"pg-dialog " + props.size} role="dialog" aria-modal="true" aria-labelledby={props.id} tabIndex={-1} ref={ref} onKeyDown={keepFocusIn}>
        <div className="pg-dialog-head">
          <h2 id={props.id}>{props.title}</h2>
          <div className="pg-dialog-actions">
            {props.actions}
            <button className="pg-icon" aria-label="Close" data-autofocus="true" onClick={props.onClose}>
              <CloseIcon />
            </button>
          </div>
        </div>
        {props.children}
      </div>
    </div>
  );
}

export function SessionsDialog(props: {
  rows: SessionRow[];
  onClose: () => void;
  onNew: () => void;
  onOpen: (id: string) => void;
  copy: (text: string, what: "reference") => void;
}) {
  return (
    <Dialog
      id="dlg-sessions"
      title="Sessions"
      size="wide"
      onClose={props.onClose}
      actions={
        <button className="pg-btn pg-primary" onClick={props.onNew}>
          New session
        </button>
      }
    >
      <div className="pg-dialog-body pg-scroll-x">
        <table className="pg-table">
          <thead>
            <tr>
              <th>Task</th>
              <th>Session</th>
              <th>Started</th>
              <th>Source</th>
              <th>Agent</th>
              <th>Variables</th>
              <th>Runs</th>
              <th>Reruns by you</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {props.rows.map((s) => (
              <tr className={s.current ? "current" : undefined} key={s.id ?? "new"}>
                <td className="pg-taskcell">
                  <span className={cls("pg-task", s.taskMuted && "pg-muted")} title={s.task}>
                    {s.task}
                  </span>
                </td>
                <td className="mono nowrap">
                  {s.copyText ? (
                    <button className="pg-cmd" title={forAsst(s.copyText)} onClick={() => props.copy(s.copyText, "reference")}>
                      {s.short}
                    </button>
                  ) : (
                    // Not started on the playground yet: nothing to copy.
                    <span className="pg-muted pg-small">{s.short}</span>
                  )}
                </td>
                <td className="nowrap">{s.started}</td>
                <td>{s.source}</td>
                <td className="pg-agentcell">
                  <div>{s.agent}</div>
                  {s.agentSettings && <small>{s.agentSettings}</small>}
                </td>
                <td className="mono">
                  {s.vars.map((v) => (
                    <div className="pg-varline" title={v} key={v}>
                      {v}
                    </div>
                  ))}
                </td>
                <td className="nowrap">
                  {s.runs}
                  {s.ends.map((e) => (
                    <span key={e.text}>
                      {" · "}
                      <span className={cls("pg-denied", e.tone)}>{e.text}</span>
                    </span>
                  ))}
                </td>
                <td className="nowrap">{s.follow}</td>
                <td className="pg-rowend nowrap">
                  {s.current && <span className="pg-muted pg-small">Open now</span>}
                  {s.openable && s.id !== null && (
                    <button className="pg-btn" onClick={() => s.id !== null && props.onOpen(s.id)}>
                      Open
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        <p className="pg-muted">Newest first. Tests and reruns aren't listed as sessions; they show under the run they reran.</p>
      </div>
    </Dialog>
  );
}

export function VarsDialog(props: { vars: FixedVar[]; settings: { name: string; value: string }[]; onClose: () => void }) {
  // One table, so the values line up; the agent's settings under their own heading.
  return (
    <Dialog id="dlg-allvars" title={props.settings.length ? "Session variables and settings" : "Session variables"} size="narrow" onClose={props.onClose}>
      <div className="pg-dialog-body">
        <table className="pg-kv">
          <tbody>
            {props.vars.map((v) => (
              <tr key={"v-" + v.name}>
                <td>{v.name}</td>
                <td className="val">{v.value}</td>
              </tr>
            ))}
            {props.settings.length > 0 && (
              <tr>
                <th colSpan={2} scope="colgroup">
                  Agent settings
                </th>
              </tr>
            )}
            {props.settings.map((p) => (
              <tr key={"s-" + p.name}>
                <td>{p.name}</td>
                <td className="val">{p.value}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Dialog>
  );
}

export interface PackageLine {
  name: string;
  /** How many rules of its own the blueprint gives it, and where they start. */
  rules: { line: number; count: number } | null;
}

export function PackagesDialog(props: {
  blueprint: string;
  inForceLabel: string;
  status: Status;
  packages: PackageLine[];
  onClose: () => void;
  onOpenRules: (name: string, line: number) => void;
}) {
  const n = props.packages.length;
  return (
    <Dialog id="dlg-pkgs" title={props.blueprint + " · " + n + (n === 1 ? " package" : " packages")} size="narrow" onClose={props.onClose}>
      <div className="pg-dialog-body">
        <p className="pg-muted">
          Packages blueprint {props.blueprint} {props.inForceLabel} lets programs import, and the rules each package has for what it may do itself.
        </p>
        <table className="pg-table">
          <thead>
            <tr>
              <th>Package</th>
              <th>Own rules</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {props.packages.map((p) => (
              <tr key={p.name}>
                <td className="mono">{p.name}</td>
                <td>{p.rules ? String(p.rules.count) : "0"}</td>
                <td className="pg-rowend">
                  {p.rules && (
                    <button className="pg-btn" onClick={() => p.rules && props.onOpenRules(p.name, p.rules.line)}>
                      Open in blueprint
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Dialog>
  );
}

export function BlueprintView(props: {
  d: BlueprintDialog;
  /** Said when the version shown isn't the one in force. */
  differs: string;
  file: string;
  onClose: () => void;
  onVersion: (v: number) => void;
  copy: (text: string, what: "reference") => void;
}) {
  const { d } = props;
  const box = useRef<HTMLDivElement>(null);
  // The lit rule comes to the middle of the text when the dialog opens or the version
  // changes; only the text scrolls, so the versions and what is lit stay in view.
  const lit = d.lines.findIndex((l) => l.active);
  useEffect(() => {
    const el = box.current;
    const active = el?.querySelector<HTMLElement>(".pg-line.active");
    // `.pg-bp` is positioned, so a line's offset is from the top of the text.
    if (el && active) el.scrollTop = active.offsetTop - el.clientHeight / 2;
  }, [d.version, lit, d.lines.length]);
  return (
    <Dialog id="dlg-bp" title={d.title} size="bp" onClose={props.onClose}>
      <div className="pg-dialog-body">
        <div className="pg-bp-versions">
          <span className="pg-muted pg-small">Versions</span>
          <div className="pg-pills" role="group" aria-label="Blueprint versions">
            {d.versions.map((v) => (
              <button className={cls("pg-pill", v.on && "on")} aria-pressed={v.on} key={v.version} onClick={() => props.onVersion(v.version)}>
                {v.name}
              </button>
            ))}
          </div>
        </div>
        <p className="pg-muted">{d.summary}</p>
        {props.differs && <p className="pg-muted">{props.differs}</p>}
        {d.focus && (
          <div className="pg-bp-focus">
            <span className="pg-muted pg-small">{d.focus.label}</span>
            <button className="pg-link" title={forAsst(d.focus.copyText)} onClick={() => d.focus && props.copy(d.focus.copyText, "reference")}>
              Copy for assistant
            </button>
          </div>
        )}
        <div className="pg-bp" ref={box}>
          {d.lines.map((l) => (
            <div className={cls("pg-line", l.active && "active")} id={"bpl-" + l.num} key={l.num}>
              <span className="num">{l.num}</span>
              <span className="txt">{l.text}</span>
            </div>
          ))}
          {!d.lines.length && <div className="pg-empty">{BP_LOADING}</div>}
        </div>
        <p className="pg-muted">
          {props.file} · printed by <code>{d.cli}</code>
        </p>
      </div>
    </Dialog>
  );
}

export function ConfirmDialog(props: { title: string; body: string; go: string; command: string; onGo: () => void; onClose: () => void }) {
  return (
    <Dialog id="dlg-confirm" title={props.title} size="narrow" onClose={props.onClose}>
      <div className="pg-dialog-body">
        <p>{props.body}</p>
        <p className="pg-muted">
          Same from the CLI: <code>{props.command}</code>
        </p>
        <div className="pg-setup-row">
          <button className="pg-btn pg-caution" onClick={props.onGo}>
            {props.go}
          </button>
          <button className="pg-btn" onClick={props.onClose}>
            Don’t run
          </button>
        </div>
      </div>
    </Dialog>
  );
}

export function DraftDialog(props: { draft: Draft; refText: string; onClose: () => void; copy: (text: string, what: "reference" | "command") => void }) {
  const { draft } = props;
  const cmd = draftCommand(draft);
  return (
    <Dialog id="dlg-draft" title={"Draft a rule for check " + draft.decision} size="bp" onClose={props.onClose}>
      <div className="pg-dialog-body">
        <p className="pg-muted">{draftWhere(draft)}</p>
        <pre className="pg-payload">{draft.untrusted.rule}</pre>
        <p className="pg-muted">{DRAFT_NOTE}</p>
        <div className="pg-actions">
          <button className="pg-link" title={forAsst(props.refText)} onClick={() => props.copy(props.refText, "reference")}>
            Copy for assistant
          </button>
          <button className="pg-copycmd" title={"Copy the CLI command: " + cmd} onClick={() => props.copy(cmd, "command")}>
            <span aria-hidden="true">⧉</span> Copy CLI command
          </button>
        </div>
      </div>
    </Dialog>
  );
}

export function MorePanel(props: { commands: string[]; focus: boolean; onFocus: () => void; onLeave: (to: Element | null) => void; onBack: () => void }) {
  const ref = useTakeFocus<HTMLDivElement>();
  return (
    <div
      className="pg-more"
      role="dialog"
      aria-label="More"
      tabIndex={-1}
      ref={ref}
      // Focus leaving it, as Tab past its last button does, closes it.
      onBlur={(ev) => !ev.currentTarget.contains(ev.relatedTarget) && props.onLeave(ev.relatedTarget instanceof Element ? ev.relatedTarget : null)}
      // It opens from More, so Shift+Tab out of it goes back to More, not to the page's end.
      onKeyDown={(ev) => {
        if (ev.key !== "Tab" || !ev.shiftKey) return;
        const first = ev.currentTarget.querySelector<HTMLElement>("button, [href], [tabindex='0']");
        if (document.activeElement === first || document.activeElement === ev.currentTarget) {
          ev.preventDefault();
          props.onBack();
        }
      }}
    >
      <h3>Use with your coding assistant</h3>
      <p>
        Every Copy for assistant gives one line: what the item is, and the command that prints it. Paste it into Claude Code or another coding
        assistant and ask for what you want, such as “why was this denied?”.
      </p>
      {props.commands.length > 0 && <pre>{props.commands.join("\n")}</pre>}
      <p>Run and decision numbers are per project and never reused, so references name the project.</p>
      <h3>About these numbers</h3>
      <p>Bytes as recorded. Reruns by you (tests and live reruns) don't count toward the session's totals.</p>
      <button className="pg-btn" onClick={props.onFocus}>
        {props.focus ? "Exit focus view" : "Focus view"}
      </button>
    </div>
  );
}

export interface Toast {
  head: string;
  body?: string;
  /** Closes itself after a few seconds; a toast that must be read or copied from stays. */
  auto: boolean;
}

export function ToastView(props: { toast: Toast; onHold: () => void; onRelease: () => void; onDismiss: () => void }) {
  return (
    <div className="pg-toast" role="status" onMouseEnter={props.onHold} onMouseLeave={props.onRelease}>
      <div className="pg-toast-text">
        <div>
          <Inline text={props.toast.head} />
        </div>
        {props.toast.body && <code className="pg-toast-body">{props.toast.body}</code>}
      </div>
      <button className="pg-link" onClick={props.onDismiss}>
        Dismiss
      </button>
    </div>
  );
}

/** A line over the panes: a notice, or something the reader should know before going on. */
export function Callout(props: { text: string; children?: ReactNode; tone?: "bad" }) {
  return (
    <div className={cls("pg-callout", props.tone)} role="note">
      <Inline text={props.text} /> {props.children}
    </div>
  );
}
