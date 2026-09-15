// expect-error: cannot be tested at a specific instantiation
class Box<T> {
  constructor(private value: T) {}
}

function main(): void {
  const u: unknown = new Box("x") as unknown;
  assert(u instanceof Box<string>);
}
