// expect-warning: caller-supplied `tags` is stored in an object literal in `send`, which calls `check()`
// expect-warning: caller-supplied `options` is stored in the property `options` in `new Client`, which calls `check()`
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

function deliver(request: { channelId: string; tags: string[] }): void {}

/**
 * Sends the message.
 * @capability test.com/send { channelId: string, count: number }
 */
export function send(input: Input): void {
  const channelId = input.channelId;
  const tags = input.tags;
  check("test.com/send", { channelId: channelId, count: tags.length });
  deliver({ channelId: channelId, tags: tags });
}

/** Sends messages. */
export class Client {
  constructor(private options: Options) {
    check("test.com/create", { unfurl: options.unfurl });
  }
}
