// An argument checked against an *already-bound* type parameter uses
// assignability, not identity: passing a `Sub` where the receiver fixed
// `T = Base` is the assignability question. The binding stays `Base`, so the
// result type is unchanged.
class Base {
  x: number = 1;
  tag(): string { return "base"; }
}

class Sub extends Base {
  y: number = 2;
  tag(): string { return "sub"; }
}

class Box<T> {
  private items: T[] = [];
  add(v: T): void { this.items.push(v); }
  first(): T | undefined { return this.items.at(0); }
  size(): number { return this.items.length; }
}

function main(): void {
  const m = new Map<Base, number>();
  m.set(new Sub(), 3);
  assert(m.size === 1, "Map#set takes a subclass instance");

  const arr: Base[] = [];
  arr.push(new Sub());
  assert(arr.length === 1, "Array#push takes a subclass instance");

  const s = new Set<Base>();
  s.add(new Sub());
  assert(s.size === 1, "Set#add takes a subclass instance");

  const box = new Box<Base>();
  box.add(new Sub());
  assert(box.size() === 1, "a user-defined generic method takes a subclass too");

  // The binding is not widened: the element still reads at the declared type,
  // and dispatch is still the instance's own.
  const held = box.first();
  assert(held !== undefined, "the element is there");
  if (held !== undefined) {
    assert(held.x === 1, "reads at the bound type `Base`");
    assert(held.tag() === "sub", "but dispatches to the runtime class");
  }

  for (const k of m.keys()) {
    assert(k.x === 1, "the Map key reads at `Base`");
  }

  // The same relaxation across the rest of the collection surface.
  const key = new Sub();
  const lookup = new Map<Base, number>();
  lookup.set(key, 7);
  assert(lookup.get(key) === 7, "Map#get takes a subclass instance");
  assert(lookup.has(key), "Map#has takes a subclass instance");
  assert(lookup.delete(key), "Map#delete takes a subclass instance");

  const set2 = new Set<Base>();
  const elem = new Sub();
  set2.add(elem);
  assert(set2.has(elem), "Set#has takes a subclass instance");
  assert(set2.delete(elem), "Set#delete takes a subclass instance");

  const list: Base[] = [];
  const tracked = new Sub();
  list.push(tracked);
  list.unshift(new Sub());
  assert(list.length === 2, "Array#unshift takes a subclass instance");
  assert(list.includes(tracked), "Array#includes takes a subclass instance");
  // Position, not identity: `indexOf` compares structurally, and the two `Sub`
  // instances have equal fields, so it finds the first.
  assert(list.indexOf(tracked) === 0, "Array#indexOf takes a subclass instance");

  // A generic *function*, not a method — same machinery.
  assert(firstOf<Base>(new Sub(), new Sub()).x === 1, "a generic function takes subclasses");

  // Three levels of inheritance, bound at the root.
  const roots: Root[] = [];
  roots.push(new Leaf());
  assert(roots.length === 1, "A <- B <- C, bound at A, passing a C");

  // An interface-typed binding receiving a class instance.
  const named: Named[] = [];
  named.push(new Person());
  assert(named.length === 1, "interface-typed binding takes a class instance");
}

class Root { a: number = 1; }
class Mid extends Root { b: number = 2; }
class Leaf extends Mid { c: number = 3; }

interface Named { label(): string; }
class Person implements Named {
  label(): string { return "p"; }
}

function firstOf<T>(a: T, b: T): T {
  return a;
}
