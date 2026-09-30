// expect-warning: unknown field `teamIdd` in `@capability` binding `$input`
// expect-error-count: 1
import { check } from "submilli:security";
import { Input } from "./types";

export { Input } from "./types";

/**
 * Runs the operation.
 * @capability test.com/op { owner: $input.teamIdd }
 */
export function run(input: Input): void {
  const owner = input.teamId;
  check("test.com/op", { owner });
}
