// A literal a pattern takes apart keeps a property's literal type where the
// property's default has literals of its kind, as in TypeScript. The bindings
// still take the default's type too, so they stay writable.
const temp = { t1: true, t2: "false" };
let [b3 = true, b4 = temp] = [false, { t1: false, t2: "hello" }];
let { x1 = true } = { x1: false };

function flip(): void {
  b3 = true;
  b4.t1 = true;
  x1 = true;
}

function main(): void {
  const { c1 = true, c2 = 0, c3 = "foo" } = { c1: false, c2: 1, c3: "bar" };
  const bit: 0 | 1 = c2;
  const word: "foo" | "bar" = c3;
  assert(!c1 && bit === 1 && word === "bar", "const bindings keep literal types");

  let { y1 = true, y2 = 0 } = { y1: false, y2: 1 };
  y1 = true;
  y2 = 7;
  assert(y1 && y2 === 7, "let bindings widen");

  assert(!b3 && !b4.t1 && !x1, "module bindings start from the literal");
  flip();
  assert(b3 && b4.t1 && x1, "and take writes of the default's type");
}
