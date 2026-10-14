// The top line: the project, the blueprint in force, its packages, and the page's menus.
import type { RefObject } from "react";

import type { Status } from "../api/types";
import type { Names } from "../model/refs";

export function Header(props: {
  names: Names;
  status: Status;
  inForce: number | null;
  focus: boolean;
  setup: boolean;
  moreOpen: boolean;
  moreRef: RefObject<HTMLButtonElement>;
  onBlueprint: () => void;
  onPackages: () => void;
  onFocus: () => void;
  onSessions: () => void;
  onNewSession: () => void;
  onMore: () => void;
}) {
  const { names, status } = props;
  const packages = status.packages;
  return (
    <header className="pg-header">
      <div className="pg-header-main">
        <div className="pg-brand">
          Submilli <span>Playground</span>
        </div>
        <span className="pg-muted pg-small">
          project <span className="pg-project">{names.project}</span>
        </span>
        <button className="pg-bplink" onClick={props.onBlueprint}>
          Blueprint {names.blueprint} · {props.inForce !== null ? "v" + props.inForce : "no version in force"}{" "}
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M3 2l3 3-3 3" fill="none" stroke="currentColor" strokeWidth="1.5"></path>
          </svg>
        </button>
        {/* One package by name; the rest are counted and listed in the dialog. */}
        <button className="pg-tag pg-pkgs" onClick={props.onPackages}>
          <span className="n">Packages</span>
          <span className="p">{packages.length ? packages[0].name : "none"}</span>
          {packages.length > 1 && <span className="c">+{packages.length - 1} more</span>}
        </button>
      </div>
      <div className="pg-header-actions">
        {props.focus && (
          <button className="pg-btn pg-exit-focus" onClick={props.onFocus}>
            Exit focus view
          </button>
        )}
        <button className="pg-btn" onClick={props.onSessions}>
          Sessions
        </button>
        {!props.setup && (
          <button className="pg-btn" onClick={props.onNewSession}>
            New session
          </button>
        )}
        <button className="pg-btn" ref={props.moreRef} aria-expanded={props.moreOpen} onClick={props.onMore}>
          More
        </button>
      </div>
    </header>
  );
}
