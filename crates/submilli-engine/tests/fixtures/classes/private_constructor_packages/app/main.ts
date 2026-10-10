import { Point, Tagged } from "@test/shapes2";

function main(): void {
  assert(Point.at(4).x === 4, "a static method builds the instance");
  assert(new Tagged(5).tag() === "x=5", "a subclass from the class's module constructs");
}
