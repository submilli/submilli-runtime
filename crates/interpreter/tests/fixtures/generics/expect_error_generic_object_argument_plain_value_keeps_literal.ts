// Two values that may be functions, however written, widen what a type
// parameter's function values return to `() => number`, as tsc's common
// supertype does; a plain literal beside one function leaves the function's
// literal return in place.
// expect-error: expected `() => 1`, got `() => number`
// expect-error: expected `1`, got `2`
// expect-error-count: 2
function pick<T>(o: { v: T; w: T | number }): T {
  return o.v;
}

const alsoOne = (): 1 => 1;

function main(): void {
  const get = pick({ v: () => 1, w: 2 });
  const one: () => 1 = get;
  assert(one() === 1, "the function keeps its literal return");
  const widened = pick({ v: (() => 1), w: alsoOne });
  const stillOne: () => 1 = widened;
  const two: 1 = 2;
}
