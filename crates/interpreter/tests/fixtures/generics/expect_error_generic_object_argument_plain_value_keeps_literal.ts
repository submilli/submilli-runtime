// Only function values typed by one type parameter widen what they return
// (tsc's common supertype of `() => 1` and `() => 2`); a plain value beside
// one function leaves the function's literal return in place.
// expect-error: expected `1`, got `2`
// expect-error-count: 1
function pick<T>(o: { v: T; w: T | number }): T {
  return o.v;
}

function main(): void {
  const get = pick({ v: () => 1, w: 2 });
  const one: () => 1 = get;
  assert(one() === 1, "the function keeps its literal return");
  const two: 1 = 2;
}
