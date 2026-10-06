// An argument decides a type parameter over the type the call's result is
// expected to have, so a result that then doesn't fit is reported, as in tsc.
// expect-error: expected `number`, got `number | null`
// expect-error: expected `string`, got `number`
// expect-error: cannot read field `length` on non-object type `number`
// expect-error: expected `Pair<T, U>`, got `{ first: number }`
// expect-error-count: 4
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

function main(): void {
  const notNull: number = orNull(5);
  const text: string = id(5);
  test({ produce: (n: number) => n, consume: (x) => x.length });
  const half: { first: number } = { first: 1 };
  second(half);
}
