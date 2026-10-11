// A later field of an object literal argument widens what an earlier one
// bound, also with a callback between them; a `new` in a field takes its type
// arguments from what the fields before it bound; and a literal after `null`
// joins the other literal candidates, as tsc infers.
class Animal {
  name: string = "a";
}

class Dog extends Animal {
  bark(): string {
    return "woof";
  }
}

function common<T>(o: { v: T; cb: (t: T) => string; w: T }): T {
  return o.w;
}

function plain<K>(o: { k: K; m: Map<K, number> }): number {
  o.m.set(o.k, 1);
  return o.m.size;
}

function pair<T>(p: { a: T; b: T }): T | null {
  return p.b;
}

class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}

class Cell<T> {
  value: T | null = null;
}

function celled<T>(o: { a: T; b: T; c: Cell<T> }): T | null {
  return o.c.value;
}

function boxed<K>(p: { k: K; h: Box<K> }): K {
  return p.h.value;
}

function three<T>(a: T, b: T, c: T): T {
  return c;
}

function main(): void {
  assert(common({ v: new Dog(), cb: (t) => t.name, w: new Animal() }).name === "a", "a later supertype field");
  assert(plain({ k: "a", m: new Map() }) === 1, "a new in a field");
  // A class with no type parameters, or a `new` with arguments, infers
  // nothing from the field, so the fields still combine.
  assert(pair({ a: new Dog(), b: null }) === null, "a non-generic class beside null");
  // A bare type parameter gives a generic `new` no type arguments to take.
  assert(pair({ a: new Cell(), b: null }) === null, "a generic class beside null");
  // A `new` that takes its type arguments from the fields before it still
  // lets a `null` field join them, in either order.
  assert(celled({ a: "x", b: null, c: new Cell() }) === null, "null after a candidate");
  assert(celled({ a: null, b: "y", c: new Cell() }) === null, "null before a candidate");
  assert(boxed({ k: null, h: new Box(new Dog()) })?.name === "a", "a new with arguments");
  const t = three(1, null, 2);
  const u: 1 | 2 | null = t;
  const v = three("a", "b", null);
  const w: "a" | "b" | null = v;
  assert(u === 2 && w === null, "literals around null");
}
