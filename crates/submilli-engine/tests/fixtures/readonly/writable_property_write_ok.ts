// A non-`readonly` interface/object-type property is writable by default
// (TS-faithful): assignment, compound assignment, and postfix all succeed.
interface Counter {
  readonly name: string;
  count: number;
}

export function main(): string {
  const c: Counter = { name: "hits", count: 0 };
  c.count = 5;
  c.count += 3;
  c.count++;
  assert(c.count === 9, "writable interface property mutated");

  const obj: { value: number } = { value: 1 };
  obj.value = 41;
  obj.value += 1;
  assert(obj.value === 42, "writable object-type property mutated");

  return c.name;
}
