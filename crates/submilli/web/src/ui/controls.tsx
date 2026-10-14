// Small controls the panes share.
import { useId, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";

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
