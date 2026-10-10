// `this` inside a closure in a *field initializer*. Initializers run as part of
// the field-setup sequence rather than a method body, so the receiver reaches
// the closure through a different path than the method/ctor/accessor cases.
class Counter {
  private step: number = 3;
  private items: number[] = [1, 2];

  scaled: number[] = this.items.map((x: number): number => x + this.step);
  doubled: number = ((): number => this.step * 2)();
  nested: number = ((): number => ((): number => this.step + 1)())();

  total(): number {
    return this.scaled[0] + this.scaled[1] + this.doubled + this.nested;
  }
}

function main(): void {
  const c = new Counter();
  assert(c.scaled[0] === 4 && c.scaled[1] === 5, "this in a field-initializer closure");
  assert(c.doubled === 6, "this in an immediately-invoked field-initializer closure");
  assert(c.nested === 4, "this through nested closures in a field initializer");
  assert(c.total() === 19, "initialized fields readable from a method");
}
