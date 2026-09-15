// Non-primitive type arguments round-trip through the erased slots: arrays,
// Map/Set, tuples, object shapes, closures, bigint, and a generic class used as
// its own type argument.
class Box<T> {
  private v: T;
  constructor(v: T) {
    this.v = v;
  }
  get(): T {
    return this.v;
  }
  set(v: T): void {
    this.v = v;
  }
}

function main(): void {
  const arr = new Box<number[]>([1, 2, 3]);
  arr.get().push(4);
  assert(arr.get()[3] === 4, "array type argument mutates in place");

  const map = new Box(new Map<string, number>());
  map.get().set("a", 1);
  assert(map.get().get("a") === 1, "Map type argument");

  const set = new Box(new Set<string>());
  set.get().add("q");
  assert(set.get().has("q"), "Set type argument");

  const tup = new Box<[number, string]>([1, "a"]);
  assert(tup.get()[1] === "a", "tuple type argument");

  const shape = new Box<{ a: number }>({ a: 2 });
  assert(shape.get().a === 2, "object-shape type argument");

  const fn = new Box<(x: number) => number>((x: number): number => x + 1);
  assert(fn.get()(1) === 2, "closure type argument");

  const big = new Box(9007199254740993n);
  assert(big.get() === 9007199254740993n, "bigint through an erased slot");

  const nested: Box<Box<number>> = new Box(new Box(7));
  assert(nested.get().get() === 7, "a generic class as its own type argument");
  nested.set(new Box(8));
  assert(nested.get().get() === 8, "written back through the erased slot");
}
