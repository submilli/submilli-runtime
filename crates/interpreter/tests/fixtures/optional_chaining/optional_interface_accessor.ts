// `?.` on an interface-typed receiver, across all three ways a property can be
// backed at runtime: a plain data slot, a `get` accessor, and nothing at all
// (an optional member on a value that reached the interface type by width
// subtyping).
//
// `ByAccessor` has to stay in this file even if it looks redundant: declaring
// `get area` anywhere in the program is what makes the absent `bagArea(absent)`
// read look accessor-backed, and without it that read never exercises the
// three-way at all.
interface Sized {
  readonly area: number;
}

class ByField implements Sized {
  area: number;
  constructor(a: number) {
    this.area = a;
  }
}

class ByAccessor implements Sized {
  private side: number;
  constructor(side: number) {
    this.side = side;
  }
  get area(): number {
    return this.side * this.side;
  }
}

interface Bag {
  tag: string;
  area?: number;
}

function optArea(s: Sized | null): number | undefined {
  return s?.area;
}

function bagArea(b: Bag | null): number | undefined {
  return b?.area;
}

function main(): void {
  assert(optArea(new ByField(9)) === 9, "field-backed interface receiver");
  assert(optArea(new ByAccessor(3)) === 9, "accessor-backed interface receiver");
  assert(optArea(null) === undefined, "short-circuit");

  const lit: Sized = { area: 4 };
  assert(optArea(lit) === 4, "object-literal receiver");

  const present: Bag = { tag: "a", area: 5 };
  assert(bagArea(present) === 5, "optional property present");

  // reaches `Bag` by width subtyping, so no `area` slot exists on the value
  const raw = { tag: "b" };
  const absent: Bag = raw;
  assert(bagArea(absent) === undefined, "optional property absent reads undefined");
  assert(bagArea(null) === undefined, "short-circuit on the optional-property shape");
}
