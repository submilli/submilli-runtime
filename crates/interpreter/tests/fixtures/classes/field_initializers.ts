// Field initializers run at construction: writable and readonly fields with
// declaration-site defaults, including a `this`-referencing initializer.
class Counter {
  count: number = 0;
  readonly label: string = "n";
  doubled: number = this.count;

  bump(): void {
    this.count = this.count + 1;
  }
}

class WithCtor {
  base: number = 10;
  scaled: number;
  constructor(factor: number) {
    this.scaled = this.base * factor;
  }
}

function main(): void {
  const c = new Counter();
  assert(c.count === 0);
  assert(c.label === "n");
  assert(c.doubled === 0);
  c.bump();
  assert(c.count === 1);

  // A writable initialized field is still assignable afterwards.
  c.count = 5;
  assert(c.count === 5);

  // Initializer runs before the constructor body, which can read it.
  const w = new WithCtor(3);
  assert(w.base === 10);
  assert(w.scaled === 30);
}
