// An array literal's elements share one type, but `null` and `undefined` widen
// it rather than conflict with it, as TypeScript infers `(number | undefined)[]`.
interface Point { x: number; y?: number; }
function main(): void {
  const p: Point = { x: 1 };
  const coords = [p.x, p.y];
  const typed: (number | undefined)[] = coords;
  assert(typed.length === 2 && coords[1] === undefined, "an optional field read");
  const gaps = [undefined, 2, 3];
  assert(gaps[0] === undefined && gaps[2] === 3, "an undefined seed takes the values after it");
  const holes = [1, null];
  assert(holes[1] === null, "null widens the same way");
}
