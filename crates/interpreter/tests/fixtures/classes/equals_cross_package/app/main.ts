import { Point } from "@test/points";
import { LabeledPoint } from "@test/labeled";

function main(): void {
  assert(new Point(1, 2) === new Point(1, 2));
  assert(new Point(1, 2) !== new Point(1, 3));

  const base: Point = new Point(1, 2);
  const sub: Point = new LabeledPoint(1, 2, "a");
  assert(base !== sub);
  assert(sub !== base);

  assert(new LabeledPoint(1, 2, "a") === new LabeledPoint(1, 2, "a"));
  assert(new LabeledPoint(1, 2, "a") !== new LabeledPoint(1, 2, "b"));
}
