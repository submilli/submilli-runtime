// An argument decides a type parameter over the type the call's result is
// expected to have, so a result that then doesn't fit is reported, as in tsc.
// expect-error: expected `number`, got `number | null`
// expect-error: expected `string`, got `number`
// expect-error: cannot read field `length` on non-object type `number`
// expect-error: expected `Pair<T, U>`, got `{ first: number }`
// expect-error: expected `1`, got `"s"`
// expect-error: expected `1 | string`, got `1 | 2`
// expect-error-count: 6
interface Pair<T, U> {
  first: T;
  second: U;
}

function test<T>(o: { produce: (n: number) => T; consume: (x: T) => number }): void {}

function second<T, U>(x: Pair<T, U>): U {
  return x.second;
}

function orNull<T>(x: T): T | null {
  return x;
}

function id<T>(x: T): T {
  return x;
}

function first<T>(a: T, b: T): T {
  return a;
}

function main(): void {
  const notNull: number = orNull(5);
  const text: string = id(5);
  test({ produce: (n: number) => n, consume: (x) => x.length });
  const half: { first: number } = { first: 1 };
  second(half);
  // The first argument decides, not the expected `number`, so only the
  // second is reported.
  const n: number = first(1, "s");
  const one: 1 | string = first(1, 2);
}
