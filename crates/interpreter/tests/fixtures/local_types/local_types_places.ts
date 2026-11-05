// A local type works in every kind of body: class members, object-literal
// methods, function expressions, catch, loop bodies and switch arms, whose
// arms share one scope.

class Counter<T> {
  private items: T[] = [];
  constructor(first: T) {
    interface Seed { item: T; }
    const seed: Seed = { item: first };
    this.items.push(seed.item);
  }
  get size(): number {
    type Size = { n: number };
    const size: Size = { n: this.items.length };
    return size.n;
  }
  set last(item: T) {
    interface Entry { item: T; }
    const entry: Entry = { item };
    this.items.push(entry.item);
  }
}

function arms<T>(n: number, t: T): T[] {
  switch (n) {
    case 0:
      interface Held { t: T; }
      return [];
    // `case 1: default:` share one body, which declares a type of its own.
    case 1:
    default:
      interface Pair<U> { held: Held; u: U; }
      return [((p: Pair<number>): T => p.held.t)({ held: { t }, u: n })];
  }
}

function enumArms(n: number): string {
  switch (n) {
    case 0:
      return "zero";
    default:
      enum Size { Small, Large }
      // A switch in an arm has a scope of its own.
      const size = n > 5 ? Size.Large : Size.Small;
      switch (size) {
        case Size.Small:
          type Label = { text: string };
          const small: Label = { text: "small" };
          return small.text;
        case Size.Large:
          return "large";
      }
  }
}

class Defaults<T> {
  value: T | undefined = ((): T | undefined => {
    interface Holder { v: T | undefined; }
    const h: Holder = { v: undefined };
    return h.v;
  })();
  static count: number = ((): number => {
    type Count = { n: number };
    const c: Count = { n: 2 };
    return c.n;
  })();
}

function methodGeneric<T>(x: T): number {
  // The method's own `T` shadows the function's.
  interface B { v: T; m<T>(t: T): T; }
  function use(b: B): number {
    return b.m<number>(3);
  }
  return 3;
}

function main(): void {
  const counter = new Counter<string>("a");
  counter.last = "b";
  assert(counter.size === 2, "constructor, getter and setter");

  const helper = {
    twice(n: number): number {
      type Doubled = { n: number };
      const d: Doubled = { n: n * 2 };
      return d.n;
    },
  };
  assert(helper.twice(4) === 8, "object-literal method");

  const named = function square(n: number): number {
    interface Squared { n: number; }
    const s: Squared = { n: n * n };
    return s.n;
  };
  assert(named(3) === 9, "named function expression");

  let caught = "";
  try {
    throw new Error("boom");
  } catch (e) {
    interface Report { text: string; }
    const r: Report = { text: e instanceof Error ? e.message : "?" };
    caught = r.text;
  }
  assert(caught === "boom", "catch");

  let total = 0;
  for (let i = 0; i < 2; i++) {
    type Step = { i: number };
    const step: Step = { i };
    total += step.i;
  }
  for (const n of [10, 20]) {
    interface Item { n: number; }
    const item: Item = { n };
    total += item.n;
  }
  assert(total === 31, "for and for-of bodies");

  assert(arms(0, "x").length === 0, "case 0");
  assert(arms(1, "y")[0] === "y", "case 1, shared with default");
  assert(arms(7, "z")[0] === "z", "default");
  assert(methodGeneric("s") === 3, "method type parameter");
  assert(enumArms(0) === "zero" && enumArms(2) === "small" && enumArms(9) === "large", "enum in an arm");
  assert(new Defaults<number>().value === undefined && Defaults.count === 2, "field initializers");
}
