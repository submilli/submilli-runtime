// A generic `main` with bounds runs: the entry call passes each bound as the
// type argument, including object types whose members the program names
// nowhere else, recursive aliases, and types inside a class's type arguments.
type Pair = { pairLeft: number, pairRight: string };
interface Nested { outer: { inner: number } }
type Tree = { label: string, kids: Tree[] };
type Index = Map<string, Index>;
class Box<T> {
  constructor(public value: T) {}
}
function main<
  T extends Pair,
  U extends Nested,
  V extends string,
  W extends Tree,
  X extends Index,
  Y extends { tags: Set<{ tagName: string }> },
  Z extends Box<{ boxed: number }>,
>(): void {
  const box = new Box<T[]>([]);
  assert(box.value.length === 0, "a generic main with bounds runs");
  const trees: W[] = [];
  assert(trees.length === 0, "a recursive bound runs");
  const indexes: X[] = [];
  assert(indexes.length === 0, "a bound inside a class's type arguments runs");
}
