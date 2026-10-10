// Generic inference that follows tsc through a data-only interface parameter
// matched against an object type, through an object literal argument's earlier
// fields, and through a nested generic call, where an argument's own type
// takes priority over the type the call's result is expected to have.

interface Opts<P, D> {
  fetch: (p: P) => D;
  map: (d: D) => string;
}
interface Pair<T, U> {
  first: T;
  second: U;
}

function example<P, D>(o: Opts<P, D>): (p: P) => string {
  return (p) => o.map(o.fetch(p));
}

function test<T>(o: { produce: (n: number) => T; consume: (x: T) => string }): string {
  return o.consume(o.produce(2));
}

function second<T, U>(x: Pair<T, U>): U {
  return x.second;
}

function orNull<T>(x: T): T | null {
  return x;
}

function maybe<T>(x: T): T | null {
  return x;
}

function id<T>(x: T): T {
  return x;
}

function nested<T>(x: T): T | null {
  return orNull(maybe(x));
}

function main(): void {
  const fetchThenMap = example({ fetch: (p: number) => p * 2, map: (n) => String(n + 1) });
  assert(fetchThenMap(3) === "7", "a later field's parameter is typed by an earlier field");

  assert(test({ produce: (n: number) => n * 3, consume: (x) => x.toFixed(1) }) === "6.0", "a sibling field's return type");
  assert(test({ produce: (n: number) => ({ v: n }), consume: (x) => String(x.v) }) === "2", "an object return type");

  const plain: { first: boolean; second: string } = { first: true, second: "s" };
  assert(second(plain).length === 1, "an object type matched against an interface parameter");
  const pair: Pair<string, number> = { first: "a", second: 1 };
  assert(second(pair) + 1 === 2, "the interface itself");

  assert(nested(4) === 4, "a nested generic call in a generic function");
  const fromInner: number | null = orNull(id<number | null>(null));
  assert(fromInner === null, "an argument wider than the expected result's binding");

  const tagged: { k: "a" | "b" } = id({ k: "a" });
  assert(tagged.k === "a", "the expected result still types a literal argument");
}
