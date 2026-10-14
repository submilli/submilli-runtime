// The chat pane: the conversation up to the cursor, and the box to send the next message.
import { useLayoutEffect, useRef, type FormEvent, type RefObject, type UIEventHandler } from "react";

import type { AskBox, ChatItem } from "../model/chat";
import { cls, Inline } from "./controls";

export function ChatPane(props: {
  items: ChatItem[];
  empty: string;
  box: AskBox;
  text: string;
  scrollRef: RefObject<HTMLDivElement>;
  inputRef?: RefObject<HTMLTextAreaElement>;
  onScroll: UIEventHandler;
  onText: (text: string) => void;
  onSend: () => void;
  onStop: () => void;
  onNew: () => void;
  onBack?: () => void;
  onPick: (run: number) => void;
}) {
  const { box } = props;
  // The box fits what's typed; its CSS caps how tall it gets.
  const own = useRef<HTMLTextAreaElement>(null);
  const ask = props.inputRef ?? own;
  useLayoutEffect(() => {
    const el = ask.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = el.scrollHeight + 2 + "px";
  }, [ask, props.text]);
  const submit = (ev: FormEvent) => {
    ev.preventDefault();
    if (!box.disabled) props.onSend();
  };
  return (
    <section className="pg-pane chat" aria-label="Chat">
      <div className="pg-pane-head">
        <h2>Chat</h2>
      </div>
      {/* Focusable, so a long conversation can be scrolled from the keyboard. */}
      <div className="pg-chat" ref={props.scrollRef} onScroll={props.onScroll} tabIndex={0} aria-label="Conversation">
        {props.items.map((m, i) => {
          switch (m.kind) {
            case "message":
              return (
                <div className={"pg-message " + m.role} key={i}>
                  <small>{m.who}</small>
                  {m.text}
                </div>
              );
            case "tool":
            case "running":
              return (
                <div className="pg-toolline" key={i}>
                  {m.text}
                </div>
              );
            case "run":
              return (
                <button className={cls("pg-chip", m.selected && "selected")} title="Show this run in the log" key={i} onClick={() => props.onPick(m.run)}>
                  {m.text}
                </button>
              );
            case "wait":
              return (
                <div className="pg-wait" key={i}>
                  <span>{m.text}</span>
                </div>
              );
            case "stopped":
              return (
                <div className="pg-toolline pg-stopped" role="status" key={i}>
                  {m.text} {m.note}
                </div>
              );
            case "failed":
              return (
                <div className="pg-chat-failed" role="alert" key={i}>
                  <div>
                    <Inline text={m.text} />
                  </div>
                  <small>{m.note}</small>
                </div>
              );
          }
        })}
        {!props.items.length && (
          <div className="pg-empty">
            <Inline text={props.empty} />
          </div>
        )}
      </div>
      {box.none ? (
        // Nothing to send into: a box that can't be typed in would only puzzle.
        <div className="pg-ask pg-ask-none">
          <p>{box.none.text}</p>
          {box.none.back && props.onBack && (
            <button className="pg-btn pg-primary" onClick={props.onBack}>
              Back to session {box.none.back}
            </button>
          )}
          {box.none.offer && (
            <button className="pg-btn pg-primary" onClick={props.onNew}>
              New session
            </button>
          )}
        </div>
      ) : (
      /* Stays focusable when it can't be used, so its reason can be read. */
      <form className="pg-ask" onSubmit={submit}>
        {/* Grows with what's typed, up to a few lines; Enter sends, Shift+Enter starts a new line. */}
        <textarea
          ref={ask}
          rows={1}
          placeholder={box.placeholder}
          readOnly={box.disabled}
          aria-disabled={box.disabled || undefined}
          title={box.reason}
          value={props.text}
          onChange={(ev) => props.onText(ev.target.value)}
          onKeyDown={(ev) => {
            // A key that ends composing an accented letter or a character isn't a send.
            if (ev.key !== "Enter" || ev.shiftKey || ev.nativeEvent.isComposing) return;
            ev.preventDefault();
            ev.currentTarget.form?.requestSubmit();
          }}
          aria-label="Message the agent"
          aria-describedby={box.reason ? "ask-reason" : undefined}
        />
        {box.stop ? (
          // While the agent answers here, the way on is to stop it.
          <button type="button" className="pg-btn" onClick={props.onStop}>
            Stop
          </button>
        ) : (
          <button className={cls("pg-btn", box.primary && "pg-primary")} aria-disabled={box.disabled || undefined} title={box.reason}>
            Send
          </button>
        )}
      </form>
      )}
      {box.reason && (
        <p id="ask-reason" className="pg-reason pg-ask-reason">
          {box.reason}
        </p>
      )}
    </section>
  );
}
