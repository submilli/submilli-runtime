// A function literal field with an unannotated parameter is inferred after
// the object literal's other fields, as in tsc, so a field after it still
// binds the type parameter its parameter reads, and the other fields bind it
// in source order to their common supertype. The fields still run in source
// order.
class Box<A> {
  constructor(public v: A) {}
}

function either(first: boolean): Box<string> | Box<number> {
  return first ? new Box("s") : new Box(1);
}

function pick<T, R>(o: { a: T | Box<boolean>; cb: (t: T) => R; z: T | Box<boolean> }): R | null {
  return null;
}

function apply<T, R>(o: { first: number; cb: (t: T) => R; value: T }): R {
  return o.cb(o.value);
}

function nested<T>(o: { v: T; cb: (t: T) => number; inner: { g: (u: T) => number } }): T {
  return o.v;
}

class Animal {
  name: string = "a";
}

class Dog extends Animal {
  bark(): string {
    return "woof";
  }
}

function common<T>(o: { v: T; cb: (t: T) => string; w: T }): T {
  return o.v;
}

let order = "";

function track<V>(tag: string, v: V): V {
  order = order + tag;
  return v;
}

function both<T>(o: { v: T; w: T }): T {
  return o.v;
}

function later<T>(a: T, o: { cb: (t: T) => number; w: T }): T {
  o.cb(a);
  return o.w;
}

interface PairOf<T> {
  v: T;
  w: T;
}

interface First<T> {
  v: T;
}

interface Extended<T> extends First<T> {
  w: T;
}

function pairOf<T>(o: PairOf<T>): T {
  return o.v;
}

function extended<T>(o: Extended<T>): T {
  return o.v;
}

function orNull<T>(o: { v: T; w: T } | null): T | null {
  return o === null ? null : o.v;
}

function optional<T>(o: { v: T; w?: T }): T {
  return o.v;
}

function main(): void {
  const picked = pick({ a: new Box(1), cb: (t) => t, z: either(true) });
  const boxes: Box<string> | Box<number> | null = picked;
  const length = apply({ first: track("a", 1), cb: (t) => t.length, value: track("b", "abc") });
  const n: number = length;
  const text: string = nested({ v: "abc", cb: (t) => t.length, inner: { g: (u) => u.length } });
  const animal: Animal = common({ v: new Dog(), cb: (t) => t.name, w: new Animal() });
  assert(boxes === null && n === 3 && order === "ab", "callback fields last");
  assert(text === "abc" && animal.name === "a", "fields before the callback");
  const unexpected = common({ v: new Dog(), cb: (t) => t.name, w: new Animal() });
  assert(unexpected.name === "a", "a later field widens an earlier one");
  const doubled = both({ v: (x: number) => x, w: (x) => x * 2 });
  const lists = common({ v: [1], cb: (t) => `${t.length}`, w: [] });
  const map = later(new Map<string, number>(), { cb: (t) => t.size, w: new Map() });
  map.set("k", 1);
  assert(doubled(2) === 2 && lists.length === 1 && map.size === 1, "an earlier binding types a later field");
  const returned = common({ v: () => 1, cb: (t) => `${t()}`, w: () => 2 });
  const returnedBoth = both({ v: () => "x", w: () => "y" });
  assert(returned() === 1 && returnedBoth() === "x", "two fields widen what they return");
  const fromInterface = pairOf({ v: () => 1, w: () => 2 });
  const fromExtended = extended({ v: () => 1, w: () => 2 });
  const fromNullable = orNull({ v: () => 1, w: () => 2 });
  const kept: () => 1 = optional({ v: () => 1 });
  assert(fromInterface() + fromExtended() === 2 && fromNullable !== null && kept() === 1, "fields of other shapes");
}
