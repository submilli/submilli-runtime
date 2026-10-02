// expect-warning: payload key `extra` missing from `@capability` binding
// expect-error-count: 1
import { check } from "submilli:security";

/**
 * Runs the operation.
 * @param id Identifier of the target.
 * @capability test.com/op { id }
 */
export function run(id: string): void {
  check("test.com/op", { id, extra: true });
}
