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

function main(): void {
  const picked = pick({ a: new Box(1), cb: (t) => t, z: either(true) });
  const boxes: Box<string> | Box<number> | null = picked;
  const length = apply({ first: track("a", 1), cb: (t) => t.length, value: track("b", "abc") });
  const n: number = length;
  const text: string = nested({ v: "abc", cb: (t) => t.length, inner: { g: (u) => u.length } });
  const animal: Animal = common({ v: new Dog(), cb: (t) => t.name, w: new Animal() });
  assert(boxes === null && n === 3 && order === "ab", "callback fields last");
  assert(text === "abc" && animal.name === "a", "fields before the callback");
}
