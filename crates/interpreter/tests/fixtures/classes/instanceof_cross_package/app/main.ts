import { Shape, Circle, Square } from "@test/shapes";
import { Rect } from "@test/rects";

function main(): void {
  const c: Shape = new Circle();
  assert(c instanceof Circle);
  assert(!(c instanceof Square));
  assert(c instanceof Shape);

  const r: Shape = new Rect(2, 3);
  assert(r instanceof Rect);
  assert(r instanceof Shape);
  assert(!(r instanceof Circle));

  const c2: Shape = new Circle();
  assert(!(c2 instanceof Rect));
}
