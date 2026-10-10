import { check } from "submilli:security";

/**
 * Gates an operation on behalf of whoever called it.
 * @capability test.com/op { foo }
 */
export function guarded(foo: number): void {
  check("test.com/op", { foo });
}
