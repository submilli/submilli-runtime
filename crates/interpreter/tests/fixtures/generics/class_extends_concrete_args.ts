// Subclass of a generic parent at concrete args: inherited methods through
// erased slots, a concrete-typed override sharing the parent's erased slot,
// super() into the generic ctor, and dispatch through a parent-typed ref.
class Box<T> {
  value: T;
  constructor(v: T) {
    this.value = v;
  }
  get(): T {
    return this.value;
  }
  describe(): string {
    return "box";
  }
}

class NumBox extends Box<number> {
  constructor(v: number) {
    super(v);
  }
  get(): number {
    return super.get() + 100;
  }
  describe(): string {
    return "numbox";
  }
}

class TinyBox extends Box<number> {}

function main(): void {
  const nb = new NumBox(5);
  assert(nb.get() === 105, "concrete override + super call on erased slot");
  assert(nb.value === 5, "inherited generic field at concrete args");

  const asBox: Box<number> = nb;
  assert(asBox.get() === 105, "vtable dispatch through parent-typed ref");
  assert(asBox.describe() === "numbox", "non-generic override still dispatches");
  assert(asBox.value + 1 === 6, "field unboxed through parent type");

  const t = new TinyBox(7);
  assert(t.get() === 7, "implicit ctor inherits generic parent params");
}
