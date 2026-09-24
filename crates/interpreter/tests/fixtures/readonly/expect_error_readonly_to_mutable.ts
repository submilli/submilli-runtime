// A readonly array or tuple is not assignable to a mutable one: the mutable
// reference could write through it. Each line matches a `tsc --strict` error.
// expect-error-count: 5
// expect-error: expected `number[]`, got `readonly number[]`
// expect-error: `readonly number[]` is `readonly` and cannot be assigned to the mutable type `number[]`
// expect-error: expected `[number, string]`, got `Pair`
// expect-error: expected `T[]`, got `readonly number[]`
type Pair = readonly [number, string];

function count(xs: number[]): number {
  return xs.length;
}

function first(pair: [number, string]): number {
  return pair[0];
}

function append<T>(xs: T[], x: T): void {
  xs.push(x);
}

function main(): void {
  const ro: readonly number[] = [1, 2, 3];
  count(ro);
  const writable: number[] = ro;
  const pair: Pair = [1, "a"];
  first(pair);
  const plain: [number, string] = pair;
  append(ro, 4);
  console.log(writable, plain);
}
