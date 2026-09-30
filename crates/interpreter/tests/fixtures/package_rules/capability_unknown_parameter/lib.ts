// expect-warning: unknown parameter binding `$owner` in `@capability`
// expect-error-count: 1
import { check } from "submilli:security";

/**
 * Runs the operation.
 * @capability test.com/op { id: $owner }
 */
export function run(id: string): void {
  check("test.com/op", { id });
}
