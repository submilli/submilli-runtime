// expect-warning: `actingAs` is read more than once in `post`, which calls `check()`
// expect-error-count: 1
import { check } from "submilli:security";

/** A message. */
export interface Input {
  /** Message text. */
  text: string;
}

let actingAs: string | null = null;

/** Choose whom later calls act as. */
export function actAs(user: string | null): void {
  actingAs = user;
}

/**
 * Posts a message, as the chosen user when one is set. A getter of `input`
 * can call `actAs` between the check and the second read of `actingAs`.
 * @capability test.com/impersonate {}
 */
export function post(input: Input): string {
  if (actingAs !== null) check("test.com/impersonate", {});
  const text = input.text;
  return (actingAs ?? "self") + ": " + text;
}

/**
 * Posts a message, reading the chosen user once.
 * @capability test.com/impersonate {}
 */
export function postOnce(input: Input): string {
  const user = actingAs;
  if (user !== null) check("test.com/impersonate", {});
  const text = input.text;
  return (user ?? "self") + ": " + text;
}
