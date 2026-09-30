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

function keep(tags: string[]): void {}

/**
 * Sends the message.
 * @capability test.com/send { channelId: string }
 */
export function send(input: Input): void {
  const channelId = input.channelId;
  check("test.com/send", { channelId: channelId });
  post(channelId, input.text);
  post(channelId, input.text);
  keep(input.tags);
  if (input.options !== null) {
    post(channelId, input.options.unfurl ? "unfurled" : "plain");
  }
}
