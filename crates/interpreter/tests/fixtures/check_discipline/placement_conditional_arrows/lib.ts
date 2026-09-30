// expect-warning: `check()` is called in a function value that no exported function or exported constant names
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

const strict: boolean = true;

/** Sends the message. */
export const send = strict
  ? (input: Input): void => {
      check("test.com/send", { channelId: input.channelId });
      post(input.channelId, "hello");
    }
  : (input: Input): void => {
      post(input.channelId, "hello");
    };
