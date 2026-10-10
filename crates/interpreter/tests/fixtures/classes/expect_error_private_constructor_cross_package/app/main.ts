// expect-error: the constructor of class `Point` is private
// expect-error: the constructor of class `Labeled2` is private
// expect-error: field `nope` does not exist
// expect-error-count: 5
import { Labeled2, Point as Shape } from "@test/shapes";

// Errors name the declared class, not the local alias. A class that may not
// extend its parent keeps it otherwise: it is still a `Shape`, and its members
// are still checked. Only the hidden constructor is left out, so neither
// `super()` nor `new Labeled3()` is held to it.
class Point3 extends Shape {
  constructor(public z: number) {
    super();
  }
}

class Labeled3 extends Labeled2 {}

function main(): void {
  const p = new Shape(3);
  const q = new Labeled2(4);
  const r = new Labeled3();
  const asParent: Shape = new Point3(1);
  assert(p.x + q.x + r.x + asParent.x === new Point3(2).nope, "unreachable");
}
