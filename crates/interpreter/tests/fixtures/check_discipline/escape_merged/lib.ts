// expect-warning: a conditional expression yields more than one caller-supplied value in `send`, which calls `check()`
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
 * @capability test.com/send { channelId: string, unfurl: boolean }
 */
export function send(channelId: string, options: Options | null, fallback: Options): void {
  const chosen = options ?? fallback;
  check("test.com/send", { channelId: channelId, unfurl: chosen.unfurl });
  post(channelId, "sent");
}
