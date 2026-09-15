// TS 4.3+: a getter and setter may have different types. Here the write type is
// `number` and the read type is `string` — they are independent.
class Counter {
  private n: number = 0;

  get label(): string {
    return this.n.toString();
  }
  set label(v: number) {
    this.n = v;
  }
}

function main(): void {
  const c = new Counter();
  c.label = 7; // setter takes a number
  const read: string = c.label; // getter returns a string
  assert(read === "7");
  c.label = 42;
  assert(c.label === "42");
}
