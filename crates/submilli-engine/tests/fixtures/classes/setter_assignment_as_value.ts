// An assignment through a setter is a value, as in JavaScript: it can chain,
// sit in a condition or a loop update, and dispatch to an overriding setter.
class Box {
  stored: number = 0;
  get value(): number { return this.stored; }
  set value(next: number) { this.stored = next; }
}
class Doubling extends Box {
  set value(next: number) { this.stored = next * 2; }
  get value(): number { return this.stored; }
}
function main(): void {
  const a = new Box();
  const b = new Box();
  a.value = b.value = 3;
  assert(a.value === 3 && b.value === 3, "a chained write");
  if ((a.value = 5) === 5) {
    assert(a.value === 5, "a write in a condition");
  }
  let steps = 0;
  for (b.value = 0; b.value < 3; b.value += 1) steps++;
  assert(steps === 3 && b.value === 3, "writes in a for header");
  const base: Box = new Doubling();
  base.value = 4;
  assert(base.value === 8, "an overriding setter through a base type");
}
