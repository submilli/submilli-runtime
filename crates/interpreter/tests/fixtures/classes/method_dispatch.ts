// Same-package static method dispatch (SUB-484): `x.m()` on a class-typed
// receiver dispatches through the class vtable, including `this.m()` self-calls.
class Counter {
  private n: number;
  constructor(start: number) {
    this.n = start;
  }
  value(): number {
    return this.n;
  }
  plus(d: number): number {
    return this.value() + d; // this.m() self-call
  }
  bump(): void {
    this.n = this.n + 1;
  }
}

class Greeter {
  private who: string;
  constructor(who: string) {
    this.who = who;
  }
  greet(): string {
    return "hi " + this.who;
  }
}

function main(): void {
  const c = new Counter(10);
  assert(c.value() === 10);
  assert(c.plus(5) === 15);
  c.bump();
  assert(c.value() === 11);

  const g = new Greeter("ada");
  assert(g.greet() === "hi ada");
}
