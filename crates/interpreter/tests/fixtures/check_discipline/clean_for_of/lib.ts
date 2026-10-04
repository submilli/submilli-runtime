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
 * @param tags Labels to attach.
 * @param routes Routes to deliver to.
 * @capability test.com/send { channelId: string }
 */
export function send(channelId: string, tags: string[], routes: Options[]): void {
  for (const tag of tags) {
    check("test.com/send", { channelId: tag });
    post(channelId, tag);
    post(channelId, tag + tag);
  }
  for (const route of routes) {
    check("test.com/send", { channelId: channelId });
    const unfurl = route.unfurl;
    for (const user of route.notify) {
      post(user, unfurl ? "unfurled" : "plain");
    }
  }
}
