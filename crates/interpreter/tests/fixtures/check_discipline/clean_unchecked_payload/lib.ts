// expect-error-count: 0
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

function forward(input: Input): void {}

/**
 * Sends the payload to the conversation the caller names.
 * @capability test.com/send { channelId: string }
 */
export function send(channelId: string, payload: Input): void {
  check("test.com/send", { channelId: channelId });
  post(channelId, payload.text);
  post(channelId, payload.text + payload.channelId);
  for (const tag of payload.tags) post(channelId, tag);
  for (const tag of payload.tags) post(channelId, tag + tag);
  forward(payload);
}
