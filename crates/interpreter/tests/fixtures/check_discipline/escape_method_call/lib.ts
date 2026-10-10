// expect-warning: caller-supplied `tags` has `join` called on it in `send`, which calls `check()`
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
 * @param tags Labels to attach.
 * @capability test.com/send { channelId: string, labels: string }
 */
export function send(channelId: string, tags: string[]): void {
  const labels = tags.join(",");
  check("test.com/send", { channelId: channelId, labels: labels });
  post(channelId, labels);
}
