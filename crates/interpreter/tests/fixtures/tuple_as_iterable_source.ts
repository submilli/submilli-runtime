// A tuple is an array at runtime and routes to `Array` for member dispatch, so it is
// accepted wherever `T[]` is — including through generic-parameter inference, which the
// plain assignability rule (`const a: number[] = pair`) does not go through.
function widen(xs: number[]): number {
  xs.push(4);
  return xs.length;
}

function main(): string {
  const pair: [number, number] = [1, 2];

  const spread = [...pair];
  assert(spread.length === 2 && spread[0] === 1 && spread[1] === 2, "spread a tuple");

  const interleaved = [0, ...pair, 3];
  assert(interleaved.length === 4 && interleaved[3] === 3, "spread a tuple among elements");

  const from = Array.from(pair);
  assert(from.length === 2 && from[1] === 2, "Array.from a tuple");

  const joined = [0].concat(pair);
  assert(joined.length === 3 && joined[2] === 2, "concat a tuple onto an array");

  // A concrete `number[]` parameter took a tuple before this change too — pinned here
  // because the callee may then grow it, which is the hole the fix deliberately keeps.
  assert(widen(pair) === 3, "a concrete `number[]` parameter takes a tuple");

  // Mixed positions contribute the union of their types.
  const mixed: [number, string] = [5, "a"];
  const mixedSpread: (number | string)[] = [...mixed];
  assert(mixedSpread.length === 2 && mixedSpread[1] === "a", "spread a mixed tuple");
  const mixedFrom: (number | string)[] = Array.from(mixed);
  assert(mixedFrom[0] === 5, "Array.from a mixed tuple");

  // A union hint still pins the literal's shape: an array literal can only be the
  // union's one array-like member.
  const nullable: [number, number] | null = [6, 7];
  assert(nullable !== null && nullable[1] === 7, "tuple hint survives a union");

  let walked = 0;
  for (const e of pair) {
    walked = walked + e;
  }
  assert(walked === 7, "for-of over a tuple visits every position");

  // Through aliases, plain and generic.
  const aliased: Pair = [8, 9];
  assert([...aliased].length === 2 && Array.from(aliased)[1] === 9, "spread through an alias");
  const generic: Twice<number> = [1, 1];
  assert([...generic].length === 2, "spread through a generic alias");

  // Nested, single-element, and null-carrying tuples.
  const nested: [[number, number], [number, number]] = [[1, 2], [3, 4]];
  assert([...nested].length === 2 && [...nested][1][0] === 3, "spread a tuple of tuples");
  const one: [number] = [1];
  assert([...one].length === 1, "spread a one-element tuple");
  const withNull: [number, null] = [1, null];
  assert([...withNull].length === 2, "spread a tuple carrying null");

  // Collection constructors take a tuple of entries / values.
  const entries: [[string, number], [string, number]] = [["a", 1], ["b", 2]];
  assert(new Map<string, number>(entries).size === 2, "a tuple of entries builds a Map");
  // `pair` was grown by `widen` above, so use a fresh tuple for the arity assertions.
  const fresh: [number, number] = [1, 2];
  assert(new Set<number>(fresh).size === 2, "a tuple of values builds a Set");

  // Generic inference from a tuple argument, and the `Array<T>` spelling.
  assert(first(fresh) === 1, "a tuple binds a generic `T[]` parameter");
  assert(count(fresh) === 2, "a tuple takes the `Array<number>` spelling of the same type");
  assert(asArray().length === 2, "a tuple returned where `number[]` is declared");

  return "ok";
}

type Pair = [number, number];
type Twice<T> = [T, T];

function first<T>(xs: T[]): T {
  return xs[0];
}

function count(xs: Array<number>): number {
  return xs.length;
}

function asArray(): number[] {
  const p: [number, number] = [1, 2];
  return p;
}
