// `readonly T[]`, `ReadonlyArray<T>`, and `readonly [A, B]` read, iterate, and
// call non-mutating methods exactly like the arrays they wrap. `readonly` is a
// compile-time view: it costs nothing at runtime and is shallow.
type Scores = readonly number[];

interface Bag {
  readonly items: readonly string[];
  pairs: readonly [string, number][];
}

class Holder {
  private readonly values: readonly number[];
  constructor(values: readonly number[]) {
    this.values = values;
  }
  total(): number {
    return this.values.reduce((sum, x) => sum + x, 0);
  }
}

function sum(xs: readonly number[]): number {
  let total = 0;
  for (const x of xs) {
    total += x;
  }
  return total;
}

function first<T>(xs: ReadonlyArray<T>): T | null {
  return xs.length > 0 ? xs[0] : null;
}

function identity<T>(x: T): T {
  return x;
}

function swap(pair: readonly [number, string]): [string, number] {
  return [pair[1], pair[0]];
}

function lengthOf(value: readonly number[] | string): number {
  if (Array.isArray(value)) {
    const list: readonly number[] = value;
    return list.length;
  }
  return -1;
}

function main(): void {
  const mutable: number[] = [3, 1, 2];
  const ro: readonly number[] = mutable;
  assert(sum(ro) === 6, "readonly parameter");
  assert(sum(mutable) === 6, "a mutable array is assignable to a readonly one");
  assert(ro.length === 3 && ro[1] === 1, "length and index reads");
  assert(ro.map((x) => x * 2).join(",") === "6,2,4", "map");
  assert(ro.filter((x) => x > 1).length === 2, "filter");
  assert(ro.slice(1).join(",") === "1,2", "slice");
  assert(ro.indexOf(2) === 2 && ro.includes(1), "indexOf and includes");
  assert(ro.concat([4]).length === 4, "concat");
  assert(ro.some((x) => x > 2) && ro.every((x) => x > 0), "some and every");
  assert(ro.find((x) => x < 3) === 1, "find");
  assert(ro.toSorted().join(",") === "1,2,3", "toSorted leaves the receiver alone");
  assert(JSON.stringify(ro) === "[3,1,2]", "stringify");
  let seen = 0;
  ro.forEach((x) => {
    seen += x;
  });
  assert(seen === 6, "forEach");

  assert(Array.from(ro).length === 3 && new Set(ro).size === 3, "Array.from and Set");
  const entries: readonly [string, number][] = [["a", 1]];
  assert(new Map(entries).get("a") === 1, "Map from readonly entries");
  assert([0].concat(ro).length === 4 && [...ro, ...ro].length === 6, "concat and spread");

  const copy = [...ro];
  copy.push(4);
  assert(copy.length === 4 && ro.length === 3, "a spread copy is mutable");
  mutable.push(4);
  assert(ro.length === 4, "readonly is a view of the same array, not a copy");

  const scores: Scores = [5, 6];
  assert(first(scores) === 5, "ReadonlyArray<T> through an alias");
  assert(identity(ro).length === 4, "a generic call keeps the argument readonly");

  const pair: readonly [number, string] = [1, "a"];
  assert(swap(pair)[0] === "a", "readonly tuple");
  const [n, s] = pair;
  assert(n === 1 && s === "a", "destructure a readonly tuple");

  const bag: Bag = { items: ["a", "b"], pairs: [["x", 1]] };
  assert(bag.items.join("") === "ab" && bag.pairs[0][1] === 1, "readonly in an interface");
  assert(new Holder([1, 2, 3]).total() === 6, "readonly class field");

  const points: readonly { x: number }[] = [{ x: 1 }];
  points[0].x = 2;
  assert(points[0].x === 2, "readonly is shallow: elements stay writable");
  const nested: readonly number[][] = [[1], [2]];
  nested[0].push(9);
  assert(nested[0].length === 2, "`readonly T[][]` leaves the inner arrays mutable");

  const make = (): readonly number[] => [1, 2];
  assert(make().length === 2, "arrow with a readonly return type");
  assert(lengthOf([1, 2, 3]) === 3 && lengthOf("ab") === -1, "Array.isArray keeps readonly");
  const u: unknown = [7];
  const cast = u as readonly number[];
  assert(cast[0] === 7, "cast to a readonly array");
}
