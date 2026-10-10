// The two halves of an accessor property inherit independently: a subclass that
// declares one half keeps the other from its nearest declaring ancestor, which is
// what the vtable already dispatches.

class GetOnly {
  get g(): string {
    return "go";
  }
}

class AddsSetter extends GetOnly {
  private w: string = "";
  set g(x: string) {
    this.w = x;
  }
  peek(): string {
    return this.w;
  }
}

class SetOnly {
  private v: string = "s";
  set g(x: string) {
    this.v = x;
  }
  peek(): string {
    return this.v;
  }
}

class AddsGetter extends SetOnly {
  get g(): string {
    return "child-get";
  }
}

// The declaring ancestor may be further than one level up.
class Top {
  get p(): number {
    return 1;
  }
}
class Middle extends Top {}
class Bottom extends Middle {
  private w: number = 0;
  set p(x: number) {
    this.w = x;
  }
  peek(): number {
    return this.w;
  }
}

// An override of one half leaves the other inherited.
class Pair {
  private v: number = 10;
  get n(): number {
    return this.v;
  }
  set n(x: number) {
    this.v = x;
  }
  peek(): number {
    return this.v;
  }
}
class OverridesGetter extends Pair {
  get n(): number {
    return 99;
  }
}

// Halves may disagree about their types — the read type is the getter's.
class StringGet {
  private v: string = "a";
  get p(): string {
    return this.v;
  }
}
class NumberSet extends StringGet {
  private n: number = 0;
  set p(x: number) {
    this.n = x;
  }
  peekN(): number {
    return this.n;
  }
}

// The inherited half's type is substituted at the bindings the declaring
// ancestor sees, reached through the child's `extends` clause.
class Holder<T> {
  private v: T;
  constructor(v: T) {
    this.v = v;
  }
  get item(): T {
    return this.v;
  }
}

class Relay<U> extends Holder<U> {
  private held: U;
  constructor(v: U) {
    super(v);
    this.held = v;
  }
  set item(x: U) {
    this.held = x;
  }
  peek(): U {
    return this.held;
  }
}

// The optional-chain read shares `class_field_read_ty`, so it has to agree with
// the plain read about which half answers.
function readVia(a: AddsSetter | null): string | undefined {
  return a?.g;
}

export function main(): string {
  const a = new AddsSetter();
  a.g = "hi";
  assert(a.g === "go", "inherited getter answers the child's own read");
  assert(a.peek() === "hi", "the child's setter ran");

  const b = new AddsGetter();
  b.g = "hi";
  assert(b.g === "child-get", "the child's getter answers");
  assert(b.peek() === "hi", "inherited setter ran");

  const c = new Bottom();
  c.p = 7;
  assert(c.p === 1, "getter two levels up still answers");
  assert(c.peek() === 7, "the child's setter ran");

  const d = new OverridesGetter();
  d.n = 5;
  assert(d.n === 99, "the overriding getter wins");
  assert(d.peek() === 5, "the inherited setter ran");
  d.n += 1;
  assert(d.peek() === 100, "compound assignment reads the getter and writes the setter");

  const e = new NumberSet();
  const s: string = e.p;
  assert(s === "a", "the read type is the inherited getter's");
  e.p = 3;
  assert(e.peekN() === 3, "the setter takes its own parameter type");

  // Through a parent-typed reference the inherited half answers the same way.
  const base: GetOnly = a;
  assert(base.g === "go", "parent-typed read agrees with the child-typed one");

  assert(readVia(a) === "go", "the optional chain reads the inherited getter too");
  assert(readVia(null) === undefined, "and still short-circuits on null");

  const rn = new Relay<number>(7);
  rn.item = 42;
  const n: number = rn.item;
  assert(n === 7, "a generic subclass reads the inherited getter at its own binding");
  assert(rn.peek() === 42, "and writes through its own setter");

  const rs = new Relay<string>("a");
  rs.item = "b";
  const t: string = rs.item;
  assert(t === "a", "a second instantiation keeps its own binding");
  assert(rs.peek() === "b", "and its own setter");

  return "ok";
}
