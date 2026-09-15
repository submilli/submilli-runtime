// expect-error: class `Box` expects 1 type argument, got 2
class Box<T> {
  constructor(private value: T) {}
}

function main(): void {
  const b: Box<string, number> = new Box("x");
  assert(b !== null);
}
