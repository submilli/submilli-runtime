// Generic class fundamentals: inferred and explicit construction, generic
// method params/returns boxing round-trip through the erased vtable slots.
class Box<T> {
  private value: T;
  constructor(v: T) {
    this.value = v;
  }
  get(): T {
    return this.value;
  }
  set(v: T): void {
    this.value = v;
  }
}

function main(): void {
  const n = new Box(41);
  n.set(n.get() + 1);
  assert(n.get() === 42, "number round-trip through erased slots");

  const s = new Box<string>("hi");
  assert(s.get() === "hi", "explicit type args");
  s.set(s.get() + "!");
  assert(s.get() === "hi!", "string round-trip");

  const b: Box<boolean> = new Box(true);
  assert(b.get(), "boolean round-trip via annotation-seeded inference");
}
