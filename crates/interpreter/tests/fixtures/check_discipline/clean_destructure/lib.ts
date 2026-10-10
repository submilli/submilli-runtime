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

/**
 * Sends the message.
 * @param input Message and where to send it.
 * @capability test.com/send { channelId: string }
 */
export function send(input: Input): void {
  const { channelId, text, threadTs } = input;
  check("test.com/send", { channelId: channelId });
  post(channelId, text);
  if (threadTs !== null) {
    post(channelId, threadTs);
    post(channelId, threadTs + text);
  }
}
