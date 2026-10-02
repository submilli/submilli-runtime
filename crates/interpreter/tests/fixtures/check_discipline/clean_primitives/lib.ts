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
 * @param channelId Conversation to post in.
 * @param text Message text.
 * @param attempts Number of delivery attempts.
 * @param urgent Whether delivery is urgent.
 * @capability test.com/send { channelId: string }
 */
export function send(channelId: string, text: string, attempts: number, urgent: boolean | null): void {
  check("test.com/send", { channelId: channelId });
  for (let attempt = 0; attempt < attempts; attempt++) {
    post(channelId, text);
    post(channelId, text + channelId);
  }
  if (urgent !== null && urgent) {
    post(channelId, "urgent: " + text);
  }
}
