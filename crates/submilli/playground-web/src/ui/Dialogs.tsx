import { packageDocsKey } from "../api/types";
// The page's dialogs and toast. Each dialog takes focus when it opens;
// the page gives focus back to whatever opened it when it closes.
import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent, type ReactNode } from "react";

import type { ActionOutcome } from "../api/client";
import type { DeclaredVariable, Draft, PackageDocs, SavedKey, Status } from "../api/types";
import type { BlueprintDialog } from "../model/blueprint";
import { forAsst } from "../model/refs";
import type { FixedVar, SessionRow } from "../model/sessions";
import { keyNote, keyNow, keySaved } from "../model/setup";
import { providerName } from "../model/format";
import { CloseIcon, cls, Inline, Markdown } from "./controls";
import { BP_LOADING, DRAFT_NOTE, draftCommand, draftWhere } from "./wording";
import "../styles/setup-dialogs.css";

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
                  {s.moreVars && (
                    <div className="pg-muted pg-small" title={s.allVars.join("\n")}>
                      {s.moreVars}
                    </div>
                  )}
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

/**
 * Adds or replaces a provider's model key: typed in a masked field and saved in the local
 * secret store, as `submilli secret put` does. The key leaves the field only in the save
 * request, and the field is emptied whatever the outcome.
 */
export function KeyDialog(props: {
  status: Status;
  provider: string;
  onSave: (provider: string, key: string | null) => Promise<ActionOutcome<SavedKey>>;
  /** After a save or removal: what to say, and which provider's key it was. */
  onDone: (notice: string, provider: string, removed: boolean) => void;
  onClose: () => void;
}) {
  const providers = (props.status.model_keys?.length ? props.status.model_keys.map((k) => k.provider) : ["anthropic", "openai", "google"]);
  const [provider, setProvider] = useState(props.provider || providers[0] || "");
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // Removing a saved key asks once more: chat can't use that provider until one is added again.
  const [removing, setRemoving] = useState(false);
  const now = keyNow(props.status, provider);
  const name = providerName(provider);
  const run = async (value: string | null) => {
    setBusy(true);
    setError("");
    const outcome = await props.onSave(provider, value);
    setBusy(false);
    setKey("");
    setRemoving(false);
    if (!outcome.ok) return setError(outcome.message);
    props.onDone(keySaved(provider, outcome.result, value === null), provider, value === null);
  };
  const submit = (ev: FormEvent) => {
    ev.preventDefault();
    if (key.trim() && !busy) void run(key);
  };
  return (
    <Dialog id="dlg-key" title="Model key" size="narrow" onClose={props.onClose}>
      <form className="pg-dialog-body pg-keyform" onSubmit={submit}>
        <div className="pg-choices" role="group" aria-label="Provider">
          {providers.map((p) => (
            <button
              type="button"
              key={p}
              className={cls("pg-btn", p === provider && "pg-primary")}
              aria-pressed={p === provider}
              onClick={() => {
                setProvider(p);
                setError("");
                setRemoving(false);
              }}
            >
              {providerName(p)}
            </button>
          ))}
        </div>
        <p id="key-now">{now.text}</p>
        {removing ? (
          <div className="pg-confirm" role="group" aria-label="Confirm removal">
            <p>
              Remove the saved {name} key? Chat can’t use {name} until you add a key again.
            </p>
            <div className="pg-setup-row">
              <button type="button" className="pg-btn pg-danger" data-autofocus="true" disabled={busy} onClick={() => void run(null)}>
                Remove key
              </button>
              <button type="button" className="pg-btn" onClick={() => setRemoving(false)}>
                Keep it
              </button>
            </div>
          </div>
        ) : (
          <>
            <label className="pg-vfield">
              <span className="pg-vname">{now.saved || now.text.startsWith("A ") ? "New " + name + " API key" : name + " API key"}</span>
              <input
                type="password"
                aria-label={name + " API key"}
                autoComplete="off"
                spellCheck={false}
                data-autofocus="true"
                value={key}
                onChange={(ev) => setKey(ev.target.value)}
                aria-describedby={"key-now key-note" + (error ? " key-error" : "")}
                aria-invalid={!!error || undefined}
              />
              {error && (
                <small id="key-error" className="pg-field-err">
                  {error}
                </small>
              )}
            </label>
            <details className="ds-help"><summary>Key storage and precedence</summary><p id="key-note"><Inline text={keyNote(props.status, provider)} /></p></details>
            <div className="pg-setup-row">
              <button type="submit" className="pg-btn pg-primary" aria-disabled={!key.trim() || busy || undefined} disabled={busy}>
                {now.saved ? "Replace key" : "Save key"}
              </button>
              {now.saved && (
                <button type="button" className="pg-btn" disabled={busy} onClick={() => setRemoving(true)}>
                  Remove saved key…
                </button>
              )}
              <button type="button" className="pg-btn" onClick={props.onClose}>
                Cancel
              </button>
            </div>
          </>
        )}
      </form>
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
  docs: Record<string, PackageDocs>;
  docsErrors: Record<string, { code: string; message: string }>;
  onLoadDocs: (name: string, blueprint: string) => void;
  onClose: () => void;
  onOpenRules: (name: string, line: number) => void;
}) {
  const n = props.packages.length;
  const [selected, setSelected] = useState(props.packages[0]?.name ?? "");
  const line = props.packages.find((p) => p.name === selected);
  const entry = props.status.packages.find((p) => p.name === selected);
  const docs = props.docs[packageDocsKey(props.blueprint, selected)];
  const error = props.docsErrors[packageDocsKey(props.blueprint, selected)];
  useEffect(() => {
    if (selected) props.onLoadDocs(selected, props.blueprint);
  }, [selected, props.blueprint, props.onLoadDocs]);
  return (
    <Dialog
      id="dlg-pkgs"
      title={"Packages in " + props.blueprint + (props.inForceLabel ? " " + props.inForceLabel : "")}
      size="bp"
      onClose={props.onClose}
    >
      <div className="pg-dialog-body">
        {n > 1 && (
          <div className="pg-pills pg-package-picker" role="group" aria-label="Package">
            {props.packages.map((p) => (
              <button
                type="button"
                key={p.name}
                className={cls("pg-pill", selected === p.name && "on")}
                aria-pressed={selected === p.name}
                onClick={() => setSelected(p.name)}
              >
                {p.name}
              </button>
            ))}
          </div>
        )}
        {entry && (
          <section className="pg-package" aria-labelledby="pg-package-name">
            <div className="pg-package-head">
              <div>
                <h3 id="pg-package-name">{entry.name}</h3>
                <p className="pg-muted">
                  v{entry.version} · {entry.project ? "Project package" : "Installed package"} · {entry.importable ? "Programs can import it" : "Dependency"}
                </p>
              </div>
              {line?.rules && (
                <button
                  className="pg-btn"
                  aria-label={`View its ${line.rules.count === 1 ? "rule" : `${line.rules.count} rules`}`}
                  onClick={() => line.rules && props.onOpenRules(line.name, line.rules.line)}
                >
                  View internal permissions ({line.rules.count} {line.rules.count === 1 ? "rule" : "rules"})
                </button>
              )}
            </div>
            <section className="pg-package-section" aria-labelledby="pg-package-api-guide">
              <h4 id="pg-package-api-guide">API guide</h4>
              <p>Authored package documentation and examples. This guide is the source of truth for the package’s public API.</p>
            {docs && (
              <div className="pg-package-docs" aria-label={entry.name + " package guide"}>
                <Markdown text={docs.untrusted.markdown} />
              </div>
            )}
            {docs?.note && <p className="pg-muted">{docs.note}</p>}
            {!docs && !error && <p className="pg-muted">Loading the package guide…</p>}
            {!docs && error && (
              <div className="pg-package-error">
                <p className="pg-error">{error.message}</p>
                <button className="pg-btn" onClick={() => props.onLoadDocs(selected, props.blueprint)}>
                  Try again
                </button>
              </div>
            )}
            </section>
            <section className="pg-package-section" aria-labelledby="pg-package-operation-rules">
              <h4 id="pg-package-operation-rules">Operation rules</h4>
              <p>Separate public operation rules are not recorded in the available package metadata. Use View blueprint to review the exact YAML and the rules that govern calls.</p>
              {line?.rules && (
                <button className="pg-link" onClick={() => props.onOpenRules(line.name, line.rules!.line)}>
                  View blueprint YAML
                </button>
              )}
            </section>
            <section className="pg-package-section" aria-labelledby="pg-package-internal-permissions">
              <h4 id="pg-package-internal-permissions">Internal permissions</h4>
              <p>
                {line?.rules
                  ? `The package has ${line.rules.count} explicit permission ${line.rules.count === 1 ? "rule" : "rules"} in its blueprint block. These describe capabilities the implementation may call.`
                  : "No explicit rules are recorded in this section. Review the blueprint YAML before drawing conclusions about access."}
              </p>
              {line?.rules && (
                <button className="pg-link" onClick={() => props.onOpenRules(line.name, line.rules!.line)}>
                  View package permission block
                </button>
              )}
            </section>
          </section>
        )}
      </div>
    </Dialog>
  );
}

export function BlueprintView(props: {
  d: BlueprintDialog;
  error: string;
  /** Said when the version shown isn't the one in force. */
  differs: string;
  file: string;
  onClose: () => void;
  onVersion: (v: number) => void;
  onRetry: () => void;
  copy: (text: string, what: "reference") => void;
}) {
  const { d } = props;
  const currentVersion = d.versions.find((v) => v.name.includes("in force"))?.version;
  const [historyOpen, setHistoryOpen] = useState(false);
  useEffect(() => {
    if (currentVersion !== undefined && currentVersion !== d.version) setHistoryOpen(true);
  }, [currentVersion, d.version]);
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
          <button className="pg-link" aria-expanded={historyOpen} onClick={() => setHistoryOpen((open) => !open)}>
            Project blueprint history ({d.versions.length})
          </button>
          {historyOpen && (
            <><p className="pg-muted">Versions from all blueprints in this local project. The in-force marker refers to the selected blueprint.</p><div className="pg-pills" role="group" aria-label="Project blueprint versions">
              {d.versions.map((v) => (
                <button className={cls("pg-pill", v.on && "on")} aria-pressed={v.on} key={v.version} onClick={() => props.onVersion(v.version)}>
                  {v.name}
                </button>
              ))}
            </div></>
          )}
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
          {!d.lines.length && !props.error && <div className="pg-empty">{BP_LOADING}</div>}
          {!d.lines.length && props.error && (
            <div className="pg-empty">
              <p>{props.error}</p>
              <button className="pg-btn" onClick={props.onRetry}>
                Try again
              </button>
            </div>
          )}
        </div>
        <p className="pg-muted">
          {props.file} · printed by <code>{d.cli}</code>
        </p>
      </div>
    </Dialog>
  );
}

type ConfirmDialogCommon = {
  title: string;
  body: string;
  go: string;
  command: string;
  onGo: () => void;
  onClose: () => void;
};

export type ConfirmDialogProps =
  | (ConfirmDialogCommon & { rerun?: undefined })
  | (ConfirmDialogCommon & {
      rerun: {
        variables: DeclaredVariable[];
        values: Record<string, string>;
        onVariable: (name: string, value: string) => void;
        error: string;
        running: boolean;
      };
    });

export function ConfirmDialog(props: ConfirmDialogProps) {
  const missing = props.rerun?.variables.filter(({ name, required }) => required && !props.rerun?.values[name]) ?? [];
  return (
    <Dialog id="dlg-confirm" title={props.title} size="narrow" onClose={props.onClose}>
      <div className="pg-dialog-body">
        <p>{props.body}</p>
        {props.rerun ? (
          <>
            <p className="pg-muted">Recorded values from this run are filled in. Review or edit them for this rerun.</p>
            {props.rerun.variables.map((variable) => (
              <label className="pg-vfield" key={variable.name}>
                {variable.name}{variable.required ? " (required)" : ""}
                <input
                  value={props.rerun.values[variable.name] ?? ""}
                  onChange={(event) => props.rerun?.onVariable(variable.name, event.target.value)}
                  placeholder={variable.example}
                  aria-required={variable.required}
                  disabled={props.rerun.running}
                />
                {variable.description && <small>{variable.description}</small>}
              </label>
            ))}
            {missing.length > 0 && <p className="pg-field-err">Enter {missing.map(({ name }) => name).join(", ")} before rerunning.</p>}
          </>
        ) : <p className="pg-muted">
          Same from the CLI: <code>{props.command}</code>
        </p>}
        {props.rerun?.error && <p role="alert" className="pg-field-err"><Inline text={props.rerun.error} /></p>}
        <div className="pg-setup-row">
          <button className="pg-btn pg-caution" disabled={missing.length > 0 || props.rerun?.running} onClick={props.onGo}>
            {props.rerun?.running ? "Rerunning…" : props.go}
          </button>
          <button className="pg-btn" onClick={props.onClose}>
            {props.rerun?.running ? "Close" : "Don’t run"}
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

export interface Toast {
  head: string;
  body?: string;
  /** Closes itself after a few seconds; a toast that must be read or copied from stays. */
  auto: boolean;
  /** What it says is running can be cancelled from here. */
  cancelable?: boolean;
  /** What it says is running can be watched, in its own session. */
  watchable?: boolean;
  /** Says this run is being cancelled, until it ends. */
  cancelling?: number;
}

export function ToastView(props: { toast: Toast; onHold: () => void; onRelease: () => void; onDismiss: () => void; onCancel?: () => void; onWatch?: () => void }) {
  return (
    <div className="pg-toast" role="status" onMouseEnter={props.onHold} onMouseLeave={props.onRelease}>
      <div className="pg-toast-text">
        <div>
          <Inline text={props.toast.head} />
        </div>
        {props.toast.body && <code className="pg-toast-body">{props.toast.body}</code>}
      </div>
      {props.toast.watchable && props.onWatch && (
        <button className="pg-btn" onClick={props.onWatch}>
          Watch it
        </button>
      )}
      {props.toast.cancelable && props.onCancel && (
        <button className="pg-btn" onClick={props.onCancel}>
          Cancel
        </button>
      )}
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
