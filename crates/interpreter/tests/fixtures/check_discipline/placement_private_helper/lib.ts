// expect-warning: `check()` is called in `guard`, which is not part of the package's public API
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
 * Approves the send.
 * @capability test.com/send { channelId: string }
 */
function guard(channelId: string): void {
  check("test.com/send", { channelId: channelId });
}

/** Sends the message. */
export function send(channelId: string, text: string): void {
  guard(channelId);
  post(channelId, text);
}
