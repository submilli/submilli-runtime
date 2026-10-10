// expect-warning: `input.text` is read inside a loop in `send`, which calls `check()`
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
 * @param input Message and where to send it.
 * @param channelIds Conversations to post in.
 * @capability test.com/send { channelId: string, text: string }
 */
export function send(input: Input, channelIds: string): void {
  check("test.com/send", { channelId: channelIds, text: input.text });
  for (let attempt = 0; attempt < 3; attempt++) {
    post(channelIds, input.text);
  }
}
