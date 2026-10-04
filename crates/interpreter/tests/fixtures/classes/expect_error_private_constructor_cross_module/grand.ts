// expect-error: expected `number`, got `string`
import { Point3 } from "./lib";

// `Point3` may not extend `Point`, but its own constructor is public and
// known, so a subclass that declares none inherits it and is checked against
// it.
/** A `Point3` with nothing added. */
export class Grand extends Point3 {}

/**
 * Constructs a `Grand` with the wrong argument type.
 * @returns Its `z`.
 */
export function wrong(): number {
  return new Grand("z").z;
}
