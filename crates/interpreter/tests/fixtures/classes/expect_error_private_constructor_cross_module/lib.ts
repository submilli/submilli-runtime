// expect-error: the constructor of class `Point` is private
// expect-error: the constructor of class `Labeled` is private
// expect-error-count: 5
import { Labeled, Point } from "./point";
import { Point as Pt } from "./point";

// `Point`'s constructor is private to module `point` (module-scoped privacy,
// spec §2.2): another module can neither call it nor extend the class. Nor can
// it call the constructor `Labeled` inherits.
/**
 * Tries to construct a `Point` directly.
 * @returns The sum of the two `x` values.
 */
export function make(): number {
  return new Point(3).x + new Labeled(4).x;
}

/** Tries to extend `Point`. */
export class Point3 extends Point {
  constructor(public z: number) {
    super(0);
  }
}

// Reported once, by its declared name, and the arguments aren't checked
// against the hidden parameters.
/**
 * Tries to construct a `Point` through an alias, with the wrong arguments.
 * @returns Its `x`.
 */
export function makeAliased(): number {
  return new Pt("a", 2).x;
}
