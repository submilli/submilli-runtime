class Counter {
  private n: number;
  constructor(n: number) {
    this.n = n;
  }
  value(): number {
    return this.n;
  }
  static origin(): Counter {
    return new Counter(0);
  }
  // A static and an instance method may share a name.
  static describe(): string {
    return "class Counter";
  }
  describe(): string {
    return "counter " + this.n.toString();
  }
  // A member literally named `static` is an ordinary method.
  static(): string {
    return "named static";
  }
}

function main(): void {
  assert(Counter.origin().value() === 0);
  assert(Counter.describe() === "class Counter");
  const c = new Counter(3);
  assert(c.describe() === "counter 3");
  assert(c.static() === "named static");
}
