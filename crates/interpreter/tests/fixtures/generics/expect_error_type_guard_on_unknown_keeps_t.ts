// A guard to `unknown` leaves a `T` a `T`: an `unknown` doesn't fit it, nor
// an array of it, as tsc reports.
// expect-error: expected `T`, got `unknown`
// expect-error: expected `T & unknown`, got `U`
// expect-error-count: 2
function isAnything(value: unknown): value is unknown {
  return true;
}

function same<V>(first: V, second: V): V {
  return second;
}

function leak<T>(t: T, u: unknown): T {
  if (isAnything(t)) {
    return same(t, u);
  }
  return t;
}

function overwrite<T, U>(t: T, u: U): T {
  if (isAnything(t)) {
    const items = [t];
    items[0] = u;
    return items[0];
  }
  return t;
}

function main(): void {
  console.log(leak<number>(1, "a"), overwrite(1, "b"));
}
