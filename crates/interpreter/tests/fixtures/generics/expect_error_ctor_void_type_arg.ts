// expect-error: `void` cannot be used as a type argument to constructor of `Box<T>`
// A written type argument on a `new` expression takes the same screen as one on
// a plain call. The inferred counterpart is `expect_error_class_infer_void_arg`.
class Box<T> {
  constructor(public value: T) {}
}

function main(): void {
  const b = new Box<void>(1);
  assert(b !== null);
}
