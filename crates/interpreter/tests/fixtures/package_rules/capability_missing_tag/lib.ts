// expect-warning: missing `@capability test.com/op` for `check()` call
// expect-error-count: 1
import { check } from "submilli:security";

/**
 * Runs the operation.
 * @param id Identifier of the target.
 */
export function run(id: string): void {
  check("test.com/op", { id });
}
