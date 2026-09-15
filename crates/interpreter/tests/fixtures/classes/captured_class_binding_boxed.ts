// A class-typed binding captured by a closure is boxed, and its box cell is
// registered before any class struct type is recorded. Cell and lookup must
// therefore agree on the erased key, or the binding's box goes missing at
// emission. Every captured *parameter* boxes, reassigned or not, so the
// parameter cases below are the common ones.
class Tag {
  constructor(readonly v: string) {}
}

class Sub extends Tag {
  constructor(v: string) {
    super(v + "!");
  }
}

class Holder {
  readonly get: () => string;
  constructor(t: Tag) {
    this.get = (): string => t.v;
    t = new Tag("replaced");
  }
}

function labels(t: Tag, xs: string[]): string[] {
  return xs.map((x: string): string => t.v + x);
}

function main(): void {
  // reassigned `let`: the closure reads through the shared cell
  let a: Tag = new Tag("one");
  const read_a = (): string => a.v;
  a = new Tag("two");
  assert(read_a() === "two", "reassigned let");

  // a subclass instance flows into a cell erased to the base class
  a = new Sub("three");
  assert(read_a() === "three!", "subclass into a base-typed cell");

  // written from inside a closure, read from another
  const write_a = (): void => {
    a = new Tag("four");
  };
  write_a();
  assert(read_a() === "four", "closure write, closure read");
  assert(a.v === "four", "closure write, direct read");

  // nullable class union — erased whole, so its cell keys the same way
  let b: Tag | null = new Tag("five");
  const read_b = (): string => (b === null ? "none" : b.v);
  assert(read_b() === "five", "nullable class union");
  b = null;
  assert(read_b() === "none", "nullable class union, null");

  // captured parameter, never reassigned
  const out = labels(new Tag("p-"), ["x", "y"]);
  assert(out[0] === "p-x", "captured param");
  assert(out[1] === "p-y", "captured param, second element");

  // captured constructor parameter, reassigned in the constructor body
  assert(new Holder(new Tag("first")).get() === "replaced", "captured ctor param");
}
