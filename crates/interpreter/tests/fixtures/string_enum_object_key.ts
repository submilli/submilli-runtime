// A string enum indexes an object whose keys are its values, as the union of
// those string literals would.
enum Key {
  A = "a",
  B = "b",
}

enum Only {
  A = "a",
}

interface Item {
  a: string;
  b: number;
}

function read(item: Item, key: Key): string | number {
  return item[key];
}

function main(): void {
  const item: Item = { a: "x", b: 2 };
  assert(read(item, Key.A) === "x", "enum member A");
  assert(read(item, Key.B) === 2, "enum member B");
  const one: string = item[Key.A];
  assert(one === "x", "a member reads its own field's type");
  const counts: Record<Key, number> = { a: 1, b: 2 };
  assert(counts[Key.B] === 2, "a Record keyed by the enum");
  const only: Only = Only.A;
  assert(item[only] === "x", "a one-member enum indexes too");
  const single: Record<Only, number> = { a: 3 };
  assert(single[Only.A] === 3, "a Record keyed by a one-member enum");
}
