// A concrete subclass of a generic base: the inherited slot's ABI is erased,
// so the subclass must reuse the parent's adapter rather than synthesize a
// concretely-typed one.
interface Container<T> {
  get(): T;
}

class Box<T> implements Container<T> {
  constructor(private v: T) {}
  get(): T {
    return this.v;
  }
}

class IntBox extends Box<number> {}

function read(c: Container<number>): number {
  return c.get();
}

function main(): void {
  assert(read(new Box<number>(3)) === 3, "generic base directly");
  assert(read(new IntBox(5)) === 5, "concrete subclass of a generic base");
}
