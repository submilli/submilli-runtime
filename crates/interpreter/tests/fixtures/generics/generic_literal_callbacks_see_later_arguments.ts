// A function literal with an unannotated parameter anywhere inside an object
// or array literal argument is typed after what the literal's other parts and
// the arguments after it bind, as tsc infers: in a nested literal, in a tuple
// inside an object, beside a spread, and before a later argument. A function
// literal beside the callback still keeps the literals it returns.
class Animal {
  name: string = "a";
}

class Dog extends Animal {
  bark(): string {
    return "woof";
  }
}

function later<T>(o: { cb: (t: T) => number }, c: T): number {
  return o.cb(c);
}

function common<T>(o: { v: T; cb: (t: T) => string }, w: T): T {
  return w;
}

function pairUp<T>(o: { p: [T, (x: T) => number] }): number {
  return o.p[1](o.p[0]);
}

function nested<T>(o: { inner: { cb: (t: T) => number }; value: T }): number {
  return o.inner.cb(o.value);
}

function apply<T, R>(o: { cb: (t: T) => R; value: T }): R {
  return o.cb(o.value);
}

function pair<T>(obj: [T, (x: T) => number]): number {
  return obj[1](obj[0]);
}

function either<T>(obj: [T, (x: T) => number] | [(x: T) => number]): number {
  return obj.length;
}

function returned<A>(o: { a: A; b: (a: A) => void }): A {
  return o.a;
}

function main(): void {
  const answer = returned({ a: () => { return 42; }, b(a) {} });
  const literal: () => 42 = answer;
  assert(literal() === 42, "a returned literal beside a callback");
  assert(later({ cb: (t) => t.length }, "abc") === 3, "a later argument");
  assert(common({ v: new Dog(), cb: (t) => t.name }, new Animal()).name === "a", "a later supertype");
  assert(pairUp({ p: ["abc", (x) => x.length] }) === 3, "a tuple inside an object");
  assert(nested({ inner: { cb: (t) => t.length }, value: "abcd" }) === 4, "a nested object");
  const base = { value: "abc" };
  assert(apply({ cb: (t) => t.length, ...base }) === 3, "a spread after the callback");
  const rest: [string] = ["abc"];
  assert(pair([...rest, (x) => x.length]) === 3, "a spread inside the tuple");
  assert(either(["ab", (x) => x.length]) === 2, "a union of tuples");
}
