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

const ROUTES: { channelId: string; owner: string }[] = [
  { channelId: "C1", owner: "ops" },
  { channelId: "C2", owner: "dev" },
];
const LIMITS = { attempts: 3 };

/**
 * Sends the message.
 * @capability test.com/send { channelId: string }
 */
export function send(channelId: string, text: string): void {
  check("test.com/send", { channelId: channelId });
  for (const route of ROUTES) {
    if (route.channelId === channelId) {
      post(route.channelId, route.owner + text);
    }
  }
  for (let attempt = 0; attempt < LIMITS.attempts; attempt++) {
    if (ROUTES.length > LIMITS.attempts) post(channelId, text);
  }
}
