// expect-error: cannot assign to `count` because it is an import
// expect-error: cannot assign to `total` because it is an import
// expect-error: cannot assign to `limit` because it is an import
import { count, total as total, bump } from "./state";
import { limit } from "./state";

/** Writes the state module's variables. */
export function update(): void {
  count = 2;
  total += 1;
  limit++;
  bump();
}
