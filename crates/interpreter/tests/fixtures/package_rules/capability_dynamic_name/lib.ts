// expect-warning: dynamic capability string in `check()`; use a string literal
// expect-error-count: 1
import { check } from "submilli:security";

/**
 * Runs the named operation.
 * @param name Name to check.
 */
export function run(name: string): void {
  check(name, {});
}
