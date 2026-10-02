// expect-warning: caller-supplied `render` is called in `send`, which calls `check()`
// expect-error-count: 1
import { check } from "submilli:security";

/** What a message is sent with. */
export interface Input {
  /** Conversation to post in. */
  channelId: string;
  /** Message text. */
  text: string;
  /** Thread to reply in. */
  threadTs: string | null;
  /** Labels to attach. */
  tags: string[];
  /** Delivery settings. */
  options: Options | null;
}

/** Delivery settings. */
export interface Options {
  /** Whether links unfurl. */
  unfurl: boolean;
  /** Users to notify. */
  notify: string[];
}

function post(channelId: string, text: string): void {}

/**
 * Sends the message.
 * @param channelId Conversation to post in.
 * @param render Produces the message text.
 * @capability test.com/send { channelId: string, text: string }
 */
export function send(channelId: string, render: () => string): void {
  const text = render();
  check("test.com/send", { channelId: channelId, text: text });
  post(channelId, text);
}
