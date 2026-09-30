// expect-warning: caller-supplied `shape` is spread into a literal in `configure`, which calls `check()`
// expect-warning: caller-supplied `tags` is spread into a literal in `label`, which calls `check()`
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

function keep(shape: { unfurl: boolean }): void {}

function join(tags: string[]): void {}

/**
 * Applies delivery settings.
 * @capability test.com/configure { unfurl: boolean }
 */
export function configure(shape: { unfurl: boolean }): void {
  const copy = { ...shape };
  check("test.com/configure", { unfurl: copy.unfurl });
  keep(copy);
}

/**
 * Attaches labels.
 * @capability test.com/label { count: number }
 */
export function label(tags: string[]): void {
  const copy = [...tags];
  check("test.com/label", { count: copy.length });
  join(copy);
}
