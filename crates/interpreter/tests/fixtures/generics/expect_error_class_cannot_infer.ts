// expect-error: cannot infer type parameter
class Box<T> {
  private value: T | null;
  constructor() {
    this.value = null;
  }
  put(v: T): void {
    this.value = v;
  }
}

function main(): void {
  const b = new Box();
  assert(b !== null);
}
