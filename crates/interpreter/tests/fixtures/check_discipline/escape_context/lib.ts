// expect-warning: caller-supplied `input` is passed to `check()` as its context in `send`, which calls `check()`
// expect-warning: caller-supplied `tags` is passed to `check()` as its context in `label`, which calls `check()`
// expect-error-count: 2
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
 * @capability test.com/send { channelId: string, text: $input.text, threadTs: $input.threadTs, tags: $input.tags, options: $input.options }
 */
export function send(input: Input): void {
  check("test.com/send", input);
}

/**
 * Labels the conversation.
 * @param tags Labels to attach.
 * @capability test.com/label { tags }
 */
export function label(tags: string[]): void {
  check("test.com/label", { tags: tags });
}
