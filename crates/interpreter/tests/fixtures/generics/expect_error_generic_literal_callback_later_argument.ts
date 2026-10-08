// The context-free parts of every argument bind a type parameter before a
// callback inside an object literal argument is typed, as in tsc. So `1`
// decides `T` here, and the field that doesn't fit it is reported, not the
// later argument.
// expect-error: expected `number | Box<number>`, got `Box<boolean>`
// expect-error-count: 1
class Box<A> {
  constructor(public v: A) {}
}

function h<T>(o: { a: T | Box<number>; cb: (t: T) => T }, c: T): T {
  return c;
}

function main(): void {
  h({ a: new Box(true), cb: (t) => t }, 1);
}
