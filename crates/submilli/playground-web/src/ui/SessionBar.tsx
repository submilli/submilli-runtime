// The line above the panes: which session is shown and what it was started with, or the
// New session form while one is being set up.
import type { Status } from "../api/types";
import type { SessionBar as Bar } from "../model/sessions";
import { forAsst } from "../model/refs";
import type { SetupForm } from "../model/setup";
import { Setup, type KeyActions } from "./Setup";
import { CancelButton } from "./controls";

function LockIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <rect x="2.5" y="5.5" width="7" height="5" rx="1" fill="none" stroke="currentColor"></rect>
      <path d="M4 5.5V4a2 2 0 014 0v1.5" fill="none" stroke="currentColor"></path>
    </svg>
  );
}

export function SessionBar(props: {
  bar: Bar | null;
  task?: string;
  setup: boolean;
  status: Status;
  form: SetupForm;
  /** The run in flight Cancel would stop, or `null`. */
  cancelRun: number | null;
  /** Cancel was pressed for this run and the server hasn't confirmed yet. */
  cancelling: boolean;
  onForm: (form: SetupForm) => void;
  onStart: () => void;
  onBlueprint: () => void;
  onPackages: () => void;
  /** Saves a model key typed in the form's key dialog, and says what it did. */
  keys: KeyActions;
  onCopy: () => void;
  onAllVars: () => void;
  onCancel: (run: number) => void;
  copy: (text: string, what: "reference" | "command") => void;
}) {
  const { bar } = props;
  return (
    <section className="pg-context" aria-label="Session">
      <span className="pg-context-label" title={props.task}>{props.setup ? "New session" : props.task || "Session"}</span>
      {!props.setup && bar && (
        <>
          {bar.vars.map((v) => (
            <span className="pg-fixed" title={v.full} key={v.name}>
              <LockIcon />
              <span className="n">{v.name}</span>
              <span className="v">{v.value}</span>
            </span>
          ))}
          {bar.moreVars && (
            <button className="pg-link" onClick={props.onAllVars}>
              {bar.moreVars}
            </button>
          )}
          <details className="pg-session-details">
            <summary>Session details</summary>
            <div className="pg-session-detail-items">
              {bar.copyText ? (
                <button className="pg-tag pg-ref" title={forAsst(bar.copyText)} onClick={props.onCopy}>
                  {bar.short} <span className="pg-copyicon" aria-hidden="true">⧉</span>
                  <span className="sr-only">Copy a line for your assistant</span>
                </button>
              ) : (
                // Not started on the playground yet, so there is nothing to copy.
                <span className="pg-tag pg-ref" title="This session starts on the playground when you send its first message.">
                  {bar.short}
                </span>
              )}
              <span className="pg-tag">{bar.source}</span>
              {bar.agent && (
                <span className="pg-tag" title="The model answering in this session">
                  Agent {bar.agent}
                </span>
              )}
              {bar.moreSettings && (
                <button className="pg-link" onClick={props.onAllVars}>
                  {bar.moreSettings}
                </button>
              )}
              <span className="pg-tag">{bar.started}</span>
            </div>
          </details>
          <span className={"pg-state" + (bar.live ? " live" : "")}>{bar.state}</span>
          {/* Cancel acts on the live run, even while the reader is stepped back. */}
          {props.cancelRun !== null && (
            <CancelButton run={props.cancelRun} cancelling={props.cancelling} onCancel={props.onCancel} />
          )}
        </>
      )}
      {props.setup && (
        <>
          <Setup status={props.status} form={props.form} onForm={props.onForm} onStart={props.onStart} onBlueprint={props.onBlueprint} onPackages={props.onPackages} keys={props.keys} />
        </>
      )}
    </section>
  );
}
