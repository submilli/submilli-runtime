// A getter step reads no stable result, but the chain still proves its
// receivers were present.
class Counter {
  private n: number = 0;
  get next(): number {
    this.n += 1;
    return this.n;
  }
  get label(): string {
    return "c" + String(this.n);
  }
}
function f(c: Counter | undefined): string {
  if (c?.next === 1) {
    return c.label;
  }
  return "none";
}
function main(): void {
  assert(f(new Counter()) === "c1");
  assert(f(undefined) === "none");
}
