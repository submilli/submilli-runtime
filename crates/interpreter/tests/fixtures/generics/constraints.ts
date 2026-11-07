function getId<T extends { id: number }>(item: T): number {
  return item.id;
}

function identity<T extends string>(value: T): T {
  return value;
}

function upper<T extends string>(value: T): string {
  return value.toUpperCase();
}

function dependent<T extends { id: number }, U extends T>(base: T, value: U): number {
  const compatible: T = value;
  return compatible.id + base.id;
}

function absent<T extends string>(): T | undefined {
  return undefined;
}

interface Item<T extends { id: number }> {
  value: T;
}

type Wrapped<T extends { id: number }> = { value: T };

class Box<T extends { id: number }> {
  value: T;
  constructor(value: T) { this.value = value; }
  id(): number { return this.value.id; }
  static choose<U extends { id: number }>(value: U): U { return value; }
}

function main(): void {
  assert(getId({ id: 42, label: "answer" }) === 42, "constrained primitive field");
  assert(identity("hello") === "hello", "constrained identity");
  assert(upper("hello") === "HELLO", "primitive receiver is unboxed");
  assert(dependent({ id: 2 }, { id: 3, name: "extra" }) === 5, "dependent constraints");
  const missing: string | undefined = absent();
  assert(missing === undefined, "uninferred constraint");
  const item: Item<{ id: number }> = { value: { id: 7 } };
  const wrapped: Wrapped<{ id: number }> = item;
  const box = new Box(wrapped.value);
  assert(box.id() === 7, "class bound");
  assert(Box.choose({ id: 9, label: "nine" }).label === "nine", "method bound");
}
