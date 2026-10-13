// The chat pane: the conversation up to the cursor, and the box to send the next message.
import type { FormEvent, RefObject, UIEventHandler } from "react";

import type { AskBox, ChatItem } from "../model/chat";
import { cls } from "./controls";

export function ChatPane(props: {
  items: ChatItem[];
  empty: string;
  box: AskBox;
  text: string;
  scrollRef: RefObject<HTMLDivElement>;
  onScroll: UIEventHandler;
  onText: (text: string) => void;
  onSend: () => void;
  onPick: (run: number) => void;
}) {
  const { box } = props;
  const submit = (ev: FormEvent) => {
    ev.preventDefault();
    if (!box.disabled) props.onSend();
  };
  return (
    <section className="pg-pane chat" aria-label="Chat">
      <div className="pg-pane-head">
        <h2>Chat</h2>
      </div>
      <div className="pg-chat" ref={props.scrollRef} onScroll={props.onScroll}>
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
          }
        })}
        {!props.items.length && <div className="pg-empty">{props.empty}</div>}
      </div>
      {/* Stays focusable when it can't be used, so its reason can be read. */}
      <form className="pg-ask" onSubmit={submit}>
        <input
          placeholder={box.placeholder}
          readOnly={box.disabled}
          aria-disabled={box.disabled || undefined}
          title={box.reason}
          value={props.text}
          onChange={(ev) => props.onText(ev.target.value)}
          aria-label="Message the agent"
        />
        <button className={cls("pg-btn", box.primary && "pg-primary")} aria-disabled={box.disabled || undefined} title={box.reason}>
          Send
        </button>
      </form>
    </section>
  );
}
