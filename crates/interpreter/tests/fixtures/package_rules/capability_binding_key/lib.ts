// expect-warning: `@capability` binding key `extra` is missing from `check()` payload
// expect-error-count: 1
import { check } from "submilli:security";

/**
 * Runs the operation.
 * @param id Operation target.
 * @capability test.com/op { id, extra: boolean }
 */
export function run(id: string): void {
  check("test.com/op", { id });
}
