// `void e` is `undefined`, so comparing it with `==` misses `null` too. The
// suggestions keep a `void` operand that may have effects, evaluated once.
// expect-error: `y == void 0` does not also match `null`: `==` compares like `===`
// expect-error: to test for either, write `y === null || y === undefined`
// expect-error: to test for `undefined` alone, write `y === undefined`
// expect-error: `z == void tick()` does not also match `null`
// expect-error: to test for either, first run `tick();` as its own statement, then write `z === null || z === undefined`
// expect-error: to test for `undefined` alone, write `z === void tick()`
// expect-error-count: 2
function tick(): number {
  return 1;
}

function main(): void {
  const y: string | null | undefined = undefined;
  const z: number | null | undefined = null;
  if (y == void 0) {}
  if (z == void tick()) {}
}
