// A function literal field with an unannotated parameter is inferred after
// the object literal's other fields, as in tsc, so a field after it still
// binds the type parameter its parameter reads. The fields still run in
// source order.
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
  assert(boxes === null && n === 3 && order === "ab", "callback fields last");
}
