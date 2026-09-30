// expect-warning: dynamic capability string in `check()`; use a string literal
// expect-error-count: 1
import { check } from "submilli:security";

/** Runs the named operation. */
export function run(name: string): void {
  check(name, {});
}
