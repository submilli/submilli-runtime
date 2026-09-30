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
 * @capability test.com/send { channelId: string }
 */
export function send(channelId: string, options: Options | null, payload: unknown): void {
  if (options !== null) {
    check("test.com/send", { channelId: channelId });
    const unfurl = options.unfurl;
    post(channelId, unfurl ? "unfurled" : "plain");
  }
  if (typeof payload === "string") {
    check("test.com/send", { channelId: channelId });
    post(channelId, payload);
    post(channelId, payload);
  }
}
