// expect-error: cannot apply `*` to `unknown`
// expect-warning: caller-supplied `amount` is used as an operand in `charge`, which calls `check()`
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

function bill(total: number): void {}

/**
 * Charges the amount.
 * @capability test.com/charge { total: number }
 */
export function charge(amount: unknown): void {
  const total = amount * 2;
  check("test.com/charge", { total: total });
  bill(total);
}
