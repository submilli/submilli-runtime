// expect-warning: caller-supplied `failure` is thrown in `send`, which calls `check()`
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

/** Why a send failed. */
export class Failure extends Error {}

/**
 * Sends the message.
 * @capability test.com/send { channelId: string, reason: string }
 */
export function send(channelId: string, failure: Failure): void {
  check("test.com/send", { channelId: channelId, reason: failure.message });
  if (channelId === "") {
    throw failure;
  }
  post(channelId, "hello");
}
