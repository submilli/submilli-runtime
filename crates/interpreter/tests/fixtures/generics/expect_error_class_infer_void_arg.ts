// expect-error: cannot infer a type argument of `void`
// Written type arguments are screened when the annotation resolves; an inferred
// one arrives through constructor inference and needs the same check.
class Box<T> {
  constructor(public value: T) {}
}

function nothing(): void {}

function main(): void {
  const b = new Box(nothing());
  assert(b !== null);
}
