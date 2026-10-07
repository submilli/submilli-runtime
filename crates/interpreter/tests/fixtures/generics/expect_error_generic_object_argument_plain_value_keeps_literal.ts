// Two values that may be functions (a parenthesized arrow, a const, an
// asserted one, a spread object's field) widen what a type parameter's
// function values return to `() => number`, as tsc's common supertype does.
// A plain value beside one function (a literal, an operator's result)
// leaves its literal return.
// expect-error: expected `() => 1`, got `() => number`
// expect-error: expected `1`, got `2`
// expect-error: expected `() => 1`, got `() => number`
// expect-error: expected `() => 1`, got `() => number`
// expect-error-count: 4
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
  const negated = pick({ v: () => 1, w: Math.random() < 2 ? -2 : 3 });
  const stillOneToo: () => 1 = negated;
  assert(stillOneToo() === 1, "an operator's value is no function");
  const spread = pick({ v: () => 1, ...{ w: alsoOne } });
  const spreadOne: () => 1 = spread;
  const maybe: (() => 1) | null = alsoOne;
  const asserted = pick({ v: () => 1, w: maybe! });
  const assertedOne: () => 1 = asserted;
  const two: 1 = 2;
}
