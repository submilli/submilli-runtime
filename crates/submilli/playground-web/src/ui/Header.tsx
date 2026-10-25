// The top line: the project, the blueprint in force, its packages, and session actions.

import type { Status } from "../api/types";
import type { Names } from "../model/refs";

export function Header(props: {
  names: Names;
  status: Status;
  inForce: number | null;
  setup: boolean;
  noSessions: boolean;
  onBlueprint: () => void;
  onPackages: () => void;
  onSessions: () => void;
  /** Leaves New session for the session shown before it, when there was one. */
  onBack: (() => void) | null;
  onNewSession: () => void;
}) {
  const { names, status } = props;
  const packages = status.packages;
  return (
    <header className="pg-header">
      <div className="pg-header-main">
        <div className="pg-brand">
          Submilli <span>Playground</span>
        </div>
        <details className="ds-help">
          <summary aria-label={"About project " + names.project}>Project <span className="pg-project">{names.project}</span></summary>
          <p>A project is the local Submilli folder and configuration opened when you launch the playground. It is not a cloud workspace created for a session.</p>
        </details>
        {!props.setup && <button className="pg-bplink" onClick={props.onBlueprint}>
          Current blueprint {names.blueprint} · {props.inForce !== null ? "v" + props.inForce : "no version in force"}{" "}
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M3 2l3 3-3 3" fill="none" stroke="currentColor" strokeWidth="1.5"></path>
          </svg>
        </button>}
        {/* One package by name; the rest are counted and listed in the dialog. */}
        {!props.setup && <button className="pg-tag pg-pkgs" onClick={props.onPackages}>
          <span className="n">Packages</span>
          <span className="p">{packages.length ? packages[0].name : "none"}</span>
          {packages.length > 1 && <span className="c">+{packages.length - 1} more</span>}
        </button>}
      </div>
      <div className="pg-header-actions">
        {props.onBack && (
          <button className="pg-btn" onClick={props.onBack}>
            ← Back to the session you were viewing
          </button>
        )}
        <button className="pg-btn" onClick={props.onSessions}>
          Sessions
        </button>
        {!props.setup && (
          <button className={props.noSessions ? "pg-btn pg-primary" : "pg-btn"} onClick={props.onNewSession}>
            New session
          </button>
        )}
      </div>
    </header>
  );
}
