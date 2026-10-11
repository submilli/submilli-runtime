// `==` compares like `===`, so a loose comparison against `null` or
// `undefined` would silently miss the other one. Both spellings are rejected
// with the comparison that tests for either.
// expect-error: `x == null` does not also match `undefined`: `==` compares like `===`
// expect-error: to test for either, write `x === null || x === undefined`
// expect-error: `y != undefined` does not also match `null`: `==` compares like `===`
// expect-error: to test for either, write `y !== null && y !== undefined`
// expect-error: `null == z` does not also match `undefined`
// expect-error-count: 3
function main(): void {
  const x: number | null | undefined = null;
  const y: string | undefined = undefined;
  const z: boolean | null = null;
  if (x == null) {}
  if (y != undefined) {}
  if (null == z) {}
}
