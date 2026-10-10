import { Mid } from "@t/mid";

class Leaf extends Mid {
  constructor(v: number) { super(v); }
  get(): number { return super.get() + 5; }
  pair(a: number, b: number): number { return a + b; }
}

function main(): void {
  const l = new Leaf(10);
  assert(l.get() === 15, "concrete override of a grandparent generic slot");
  assert(l.pair(2, 3) === 5, "two-arg concrete override");
  assert(l.extra() === 1, "mid method");
  assert(l.shown === 10, "inherited generic accessor");
}
