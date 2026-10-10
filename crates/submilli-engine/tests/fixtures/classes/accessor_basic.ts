// Getter/setter on a class-typed receiver: a computed getter (no backing field)
// and a setter that updates backing state read back through the getter.
class Temperature {
  private celsius: number = 0;

  get fahrenheit(): number {
    return this.celsius * 9 / 5 + 32;
  }

  set fahrenheit(f: number) {
    this.celsius = (f - 32) * 5 / 9;
  }

  get raw(): number {
    return this.celsius;
  }
}

function main(): void {
  const t = new Temperature();
  assert(t.fahrenheit === 32);
  t.fahrenheit = 212;
  assert(t.raw === 100);
  assert(t.fahrenheit === 212);
}
