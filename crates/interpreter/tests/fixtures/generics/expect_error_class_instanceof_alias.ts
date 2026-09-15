// expect-error: cannot be tested at a specific instantiation
// An alias names an instantiation, so testing against it would claim a check
// the tag-only runtime walk never makes.
class Box<T> {
  constructor(private value: T) {}
  get(): T {
    return this.value;
  }
}

type BoxOfNumber = Box<number>;

function main(): void {
  const x: unknown = new Box("s") as unknown;
  assert(x instanceof BoxOfNumber);
}
