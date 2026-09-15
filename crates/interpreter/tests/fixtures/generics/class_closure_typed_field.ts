// A closure-typed field on a generic class. Its signature needs a rec-group
// type even though no closure literal of that shape appears anywhere else.
class Holder<T> {
  value: T;
  combine: (a: number, b: number, c: number) => number;

  constructor(value: T) {
    this.value = value;
    this.combine = (a: number, b: number, c: number): number => a + b + c;
  }
}

function main(): void {
  const h = new Holder<string>("x");
  assert(h.combine(1, 2, 3) === 6, "closure-typed field on a generic class");
  assert(h.value === "x", "T field alongside it");
}
