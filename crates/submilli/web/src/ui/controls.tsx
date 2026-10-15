// Small controls the panes share.
import { useId, useMemo, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";

import type { ValueChange, ValuesNote } from "../model/format";
import { parseMarkdown, type Span } from "../model/markdown";

/**
 * A row header's handler: it toggles on click, Enter or Space, but not on the click that
 * ends a text selection, so recorded text in a header can still be selected and copied.
 */
export function press(fn: () => void) {
  return (ev: MouseEvent | KeyboardEvent) => {
    if (ev.type === "keydown") {
      const key = (ev as KeyboardEvent).key;
      if (key !== "Enter" && key !== " ") return;
      ev.preventDefault();
      fn();
      return;
    }
    const selection = window.getSelection?.();
    if (selection && !selection.isCollapsed && String(selection).trim()) return;
    fn();
  };
}

/**
 * A button that can be off for a reason. It stays focusable (`aria-disabled`, not
 * `disabled`) and says why, in its hover text and in a line shown while it has focus.
 */
export function Gated(props: {
  blocked: string | null;
  /** A line already on screen that gives the reason, so it isn't shown twice. */
  reasonId?: string;
  className?: string;
  title?: string;
  type?: "button" | "submit";
  /** For a button that is one of a set of choices. */
  pressed?: boolean;
  onClick?: () => void;
  /** Pressed while blocked: it still does nothing, but the caller may show more of why. */
  onBlocked?: () => void;
  children: ReactNode;
}) {
  const own = useId();
  const { blocked } = props;
  const id = props.reasonId ?? own;
  return (
    <>
      <button
        type={props.type ?? "button"}
        className={props.className}
        title={blocked ?? props.title}
        aria-pressed={props.pressed}
        aria-disabled={blocked ? true : undefined}
        aria-describedby={blocked ? id : undefined}
        onClick={(ev) => {
          if (blocked) {
            ev.preventDefault();
            props.onBlocked?.();
            return;
          }
          props.onClick?.();
        }}
      >
        {props.children}
      </button>
      {blocked && !props.reasonId && (
        <span id={id} className="pg-reason">
          {blocked}
        </span>
      )}
    </>
  );
}

/**
 * Text that names commands between backticks, as the playground's own messages do:
 * each command shows as code rather than with its backticks.
 */
export function Inline({ text }: { text: string }) {
  const parts = text.split("`");
  // An odd backtick out leaves the text as it is.
  if (parts.length % 2 === 0) return <>{text}</>;
  return <>{parts.map((p, i) => (i % 2 ? <code key={i}>{p}</code> : p))}</>;
}

/** Markdown the agent wrote, drawn as text: see `parseMarkdown`, which never yields HTML. */
export function Markdown({ text }: { text: string }) {
  // The chat pane draws again on every event and keystroke; a message's text doesn't change.
  const blocks = useMemo(() => parseMarkdown(text), [text]);
  return (
    <div className="pg-md">
      {blocks.map((b, i) => {
        switch (b.kind) {
          case "paragraph":
            return <p key={i}>{spans(b.spans)}</p>;
          case "heading":
            return (
              <p className="pg-md-head" key={i}>
                {spans(b.spans)}
              </p>
            );
          case "pre":
            return <pre key={i}>{b.text}</pre>;
          case "list": {
            const items = b.items.map((item, j) => <li key={j}>{spans(item)}</li>);
            return b.ordered ? (
              <ol key={i} start={b.start}>
                {items}
              </ol>
            ) : (
              <ul key={i}>{items}</ul>
            );
          }
        }
      })}
    </div>
  );
}

function spans(list: Span[]): ReactNode[] {
  return list.map((s, i) => {
    switch (s.kind) {
      case "text":
        return s.text;
      case "code":
        return <code key={i}>{s.text}</code>;
      case "strong":
        return <strong key={i}>{spans(s.children)}</strong>;
      case "em":
        return <em key={i}>{spans(s.children)}</em>;
    }
  });
}

/** Cancel for a running run; once pressed it waits for the playground to confirm. */
export function CancelButton(props: { run: number; cancelling: boolean; onCancel: (run: number) => void }) {
  return (
    <button
      className="pg-btn pg-caution"
      title={"Stops run " + props.run + ". It ends as cancelled once its calls drain."}
      aria-disabled={props.cancelling || undefined}
      onClick={() => !props.cancelling && props.onCancel(props.run)}
    >
      {props.cancelling ? "Cancelling run " + props.run + "…" : "Cancel run " + props.run}
    </button>
  );
}

export function CloseIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
      <path d="M3 3l8 8M11 3l-8 8" stroke="currentColor" strokeWidth="1.6"></path>
    </svg>
  );
}

export function Arrow() {
  return <span className="pg-arrow" aria-hidden="true"></span>;
}

export function cls(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(" ");
}

const changeText = (c: ValueChange) => c.name + " " + c.was + " → " + c.now;

/** The first value that differs, and a "+N more" that opens the rest in place. */
export function Values({ v }: { v: ValuesNote }) {
  const [open, setOpen] = useState(false);
  const [first, ...rest] = v.changes;
  return (
    <div className="pg-note">
      <p>
        {v.lead}
        {changeText(first)}
        {rest.length > 0 && (
          <>
            {" "}
            <button className="pg-link pg-inline" aria-expanded={open} onClick={() => setOpen(!open)}>
              {open ? "show less" : "(+" + rest.length + " more)"}
            </button>
          </>
        )}
        {v.tail}
      </p>
      {open && (
        <ul className="pg-values">
          {rest.map((c) => (
            <li key={c.name}>{changeText(c)}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
