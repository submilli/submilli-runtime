// expect-error: `never` cannot be used as a type argument to class `Box`
class Box<T> {
  constructor(private value: T) {}
}

function main(): void {
  const b: Box<never> | null = null;
  assert(b === null);
}
