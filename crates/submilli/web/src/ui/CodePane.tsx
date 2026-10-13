// The code pane: the program shown, or the package's own source at a package step.
import { useEffect, useRef } from "react";

import type { CodePane as Pane } from "../model/code";
import { forAsst } from "../model/refs";
import { cls } from "./controls";

export function CodePane(props: {
  pane: Pane;
  /** While the package's source loads, the line to show instead of its absence. */
  sourceLoading: string;
  /** Why the package's source isn't shown, in the server's words. */
  sourceError: string;
  /** What moves the lit line: the reader's picks and the cursor. */
  sel: string | null;
  program: number | null;
  cursor: string | null;
  onCopy: (text: string) => void;
}) {
  const { pane } = props;
  const box = useRef<HTMLDivElement>(null);
  // The cursor the lit line was last centred for. A step brings its line to the middle,
  // even when the line only appears once the package's source has loaded; a pick in the
  // log just brings it into view.
  const centred = useRef<string | null | undefined>(undefined);
  const lines = pane.lines.length;
  useEffect(() => {
    const active = box.current?.querySelector(".pg-line.active");
    if (!active) return;
    const moved = centred.current !== props.cursor;
    centred.current = props.cursor;
    active.scrollIntoView?.({ block: moved ? "center" : "nearest" });
  }, [props.sel, props.program, props.cursor, lines, pane.sub]);

  return (
    <section className="pg-pane code" aria-label="Code">
      <div className="pg-pane-head">
        <h2>
          Code<span className="sub">{pane.sub}</span>
        </h2>
        <div className="pg-pane-tools">
          {pane.programRef && (
            <button
              className="pg-icon"
              title={forAsst(pane.programRef)}
              aria-label="Copy a line about this program for your assistant"
              onClick={() => pane.programRef && props.onCopy(pane.programRef)}
            >
              <span aria-hidden="true">⧉</span>
            </button>
          )}
        </div>
      </div>
      {pane.codeNote && <div className="pg-code-note">{pane.codeNote}</div>}
      {pane.stepNote && <div className="pg-code-note">{pane.stepNote}</div>}
      <div className={cls("pg-code", pane.isPackage && "pkg")} aria-label="Code" ref={box}>
        {pane.lines.map((l) => (
          <div className={cls("pg-line", l.active && "active")} key={l.num}>
            <span className="num">{l.num}</span>
            <span className="txt">{l.text}</span>
          </div>
        ))}
        {pane.empty !== null && (
          <div className="pg-empty">
            <div>{props.sourceLoading || pane.empty}</div>
            {props.sourceError && <div className="pg-muted">{props.sourceError}</div>}
          </div>
        )}
      </div>
    </section>
  );
}
