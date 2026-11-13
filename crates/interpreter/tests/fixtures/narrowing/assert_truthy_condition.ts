// `asserts c` narrows the way its argument does as a condition, as Node's
// `assert(value, message)` helpers are used.
function check(c: boolean, message: string): asserts c {
  if (!c) throw new Error(message);
}

class Point {
  constructor(public x: number) {}
}

function main(): void {
  const v: unknown = "abc";
  check(typeof v === "string", "string");
  assert(v.length === 3, "typeof narrows");

  const o: { n: number } | null = { n: 2 };
  check(o !== null, "present");
  assert(o.n === 2, "null removed");

  const p: Point | string = new Point(4);
  check(p instanceof Point, "point");
  assert(p.x === 4, "instanceof narrows");
}
