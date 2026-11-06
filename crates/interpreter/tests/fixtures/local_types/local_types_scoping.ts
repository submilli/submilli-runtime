// Local types follow block scoping: each function's `Row` is its own type, a
// local type shadows an outer one of the same name, and a block's types are
// visible from its start.

interface Row { top: number; }

function first(): number {
  interface Row { a: number; }
  const r: Row = { a: 1 };
  return r.a;
}

function second(): string {
  interface Row { b: string; }
  const r: Row = { b: "two" };
  return r.b;
}

function nested(): number {
  const outer: Row = { top: 10 };
  let inner = 0;
  {
    // Used before its declaration in the same block.
    const early: Pair = [1, 2];
    type Pair = [number, number];
    interface Row { depth: number; }
    const r: Row = { depth: early[0] + early[1] };
    inner = r.depth;
  }
  return outer.top + inner;
}

class Shape {
  kind: string = "shape";
}

function classify(x: unknown): string {
  // A local type doesn't hide the class `instanceof` names.
  interface Shape { sides: number; }
  const square: Shape = { sides: 4 };
  if (x instanceof Shape) {
    return `${x.kind} ${square.sides}`;
  }
  return "other";
}

function main(): void {
  assert(first() === 1, "first Row");
  assert(second() === "two", "second Row");
  assert(nested() === 13, "inner Row shadows the top-level one");
  assert(classify(new Shape()) === "shape 4", "instanceof names the class");
  const top: Row = { top: 5 };
  assert(top.top === 5, "the top-level Row is untouched");
  if (top.top > 0) {
    type Label = { text: string };
    const label: Label = { text: "in a top-level block" };
    assert(label.text.length > 0, "a type local to a top-level block");
  }
}
