function each<T extends { id: number }>(xs: T[], read: (value: T) => number): number {
  return read(xs[0]);
}
function callable<F extends (value: { id: number }) => { id: number }>(read: F): F { return read; }
function dependentCallable<G extends (value: number) => number, F extends G>(read: F): F { return read; }
function main(): void {
  assert(each([{ id: 1, n: "a" }], value => value.id) === 1, "constraint does not reject extra fields");
  assert(each([{ id: 1, n: "a" }], value => value.n.length) === 1, "callback sees inferred extra fields");
  const read = callable(value => ({ id: value.id, n: "a" }));
  assert(read({ id: 1 }).n === "a", "callable bound types inputs and preserves inferred outputs");
  const dependent = dependentCallable(value => value + 1);
  assert(dependent(41) === 42, "dependent callable bound types callback inputs");
}
