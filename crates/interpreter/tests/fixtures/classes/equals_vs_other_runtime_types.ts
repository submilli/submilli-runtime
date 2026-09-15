// Class instances never equal non-class values — in either operand order.
// The structural-object equals bodies reject nominal (class-vtable) operands,
// and the class bodies reject anything whose vtable isn't their singleton.
class Point {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

function main(): void {
  const p: unknown = new Point(1, 2);
  const o: unknown = { x: 1, y: 2 };
  assert(p !== o);
  assert(o !== p);

  const n: unknown = 42;
  const s: unknown = "hi";
  assert(p !== n);
  assert(n !== p);
  assert(p !== s);
  assert(s !== p);

  const e: unknown = new Error("x");
  const eShape: unknown = { message: "x", name: "Error" };
  assert(e !== eShape);
  assert(eShape !== e);
}
