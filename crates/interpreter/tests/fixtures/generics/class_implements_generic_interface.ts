// A generic class implementing a generic interface at its own type param;
// calls through the interface type ride the closure-ABI adapters.
interface Container<T> {
  get(): T;
  put(v: T): void;
}

class Box<T> implements Container<T> {
  private value: T;
  constructor(v: T) {
    this.value = v;
  }
  get(): T {
    return this.value;
  }
  put(v: T): void {
    this.value = v;
  }
}

function bump(c: Container<number>): number {
  c.put(c.get() + 1);
  return c.get();
}

function main(): void {
  const b = new Box(10);
  assert(bump(b) === 11, "boxed round-trip through the interface adapter");
  assert(b.get() === 11, "mutation visible on the class instance");

  const c: Container<string> = new Box("a");
  c.put(c.get() + "b");
  assert(c.get() === "ab", "string instantiation through the interface");
}
