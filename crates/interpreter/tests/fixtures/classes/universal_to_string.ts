// Class universal toString (vtable slot 0): "[object Object]" by default, on
// both the static method-call path and the dynamic vtable path (String(x) on
// unknown, template interpolation). A user `toString(): string` method fills
// the slot instead, and a subclass inherits the parent's body through its own
// vtable.
class Point {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

class Money {
  amount: number;
  currency: string;

  constructor(amount: number, currency: string) {
    this.amount = amount;
    this.currency = currency;
  }

  toString(): string {
    return this.amount.toString() + " " + this.currency;
  }
}

class Euro extends Money {
  constructor(amount: number) {
    super(amount, "EUR");
  }
}

function main(): void {
  const p: Point = new Point(1, 2);
  assert(p.toString() === "[object Object]");
  assert(String(p) === "[object Object]");
  const u: unknown = p;
  assert(String(u) === "[object Object]");
  assert(`p is ${p}` === "p is [object Object]");

  const m: Money = new Money(5, "USD");
  assert(m.toString() === "5 USD");
  assert(String(m) === "5 USD");
  const mu: unknown = m;
  assert(String(mu) === "5 USD");
  assert(`${m}!` === "5 USD!");

  const e: Euro = new Euro(3);
  assert(e.toString() === "3 EUR");
  const eu: unknown = e;
  assert(String(eu) === "3 EUR");
}
