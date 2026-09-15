// A parenthesized type groups: `(` in type position is a function-type parameter
// list only when a `=>` follows the matching `)`.
type Pair = [number, number];

function first(f: (() => number) | null): number {
  return f === null ? -1 : f();
}

class Holder {
  fn: (() => number) | null = null;
  labels: (string | null)[] = [];
}

function main(): void {
  const nullable: (string | null)[] = ["a", null, "b"];
  assert(nullable.length === 3, "parenthesized union element type");
  assert(nullable[1] === null, "null element survives");

  const fns: (() => number)[] = [(): number => 1, (): number => 2];
  assert(fns[0]() + fns[1]() === 3, "array of parenthesized function types");

  const nested: ((n: number) => number)[] = [(n: number): number => n + 1];
  assert(nested[0](2) === 3, "parenthesized function type with a parameter");

  assert(first(null) === -1, "null branch of a parenthesized function union");
  assert(first((): number => 9) === 9, "value branch");

  const grid: (number[])[] = [[1], [2, 3]];
  assert(grid[1].length === 2, "redundant parens around an array type");

  const h = new Holder();
  assert(h.fn === null, "class field with a parenthesized union type");
  h.labels.push(null);
  assert(h.labels.length === 1, "class field with a parenthesized element type");

  const pairs: Pair[] = [[1, 2]];
  assert(pairs[0][1] === 2, "unrelated tuple alias still parses");

  const plain: (number) = 5;
  assert(plain === 5, "parens around a single type");

  // An arrow's return-type annotation ending in a group: the `=>` right after the
  // `)` belongs to the arrow body, so the parens must not read as a param list.
  const maybe = (): (string | null) => null;
  assert(maybe() === null, "grouped return type followed by the arrow body");

  const mk = (): (() => number) => (): number => 7;
  assert(mk()() === 7, "grouped function-type return annotation");

  const mkArr = (): (() => number)[] => [(): number => 8];
  assert(mkArr()[0]() === 8, "grouped array return annotation");

  // Every other type position accepts a group too.
  const tup: [(number), (string)] = [1, "a"];
  assert(tup[0] === 1, "parenthesized tuple elements");
  const obj: ({ x: number }) = { x: 1 };
  assert(obj.x === 1, "grouped object type");
  const m = new Map<(string), (number | null)>();
  m.set("a", null);
  assert(m.get("a") === null, "grouped Map type arguments");
  const u: unknown = 5;
  assert((u as (number)) === 5, "grouped `as` target");
  assert(withParens(1) === "1", "grouped parameter and return");
  assert(rest(1, 2) === 2, "grouped rest-element type");
  assert(deflt() === 3, "grouped default-parameter type");
  const b = new Cell<(number | null)>(null);
  assert(b.v === null, "grouped class type argument");
  const g: Grouped = { x: 4 };
  assert(g.x === 4, "grouped interface property type");
  const ga: GroupedAlias = 6;
  assert(ga === 6, "grouped type-alias body");
}

function withParens(x: (number)): (string) {
  return x.toString();
}

function rest(...xs: (number)[]): number {
  return xs.length;
}

function deflt(x: (number) = 3): number {
  return x;
}

interface Grouped {
  x: (number);
}

type GroupedAlias = (number);

class Cell<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
}
