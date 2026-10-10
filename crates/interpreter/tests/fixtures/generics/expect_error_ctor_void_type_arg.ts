// expect-error: expected `void`, got `number`
// A valid void type argument does not admit a numeric constructor argument.
class Box<T> {
  constructor(public value: T) {}
}

function main(): void {
  const b = new Box<void>(1);
  assert(b !== null);
}
