// Parentheses do not hide a nullish literal from the loose-comparison check.
// expect-error: `x == (null)` does not also match `undefined`: `==` compares like `===`
// expect-error: to test for either, write `x === null || x === undefined`
// expect-error: `(undefined) != y` does not also match `null`: `==` compares like `===`
// expect-error: to test for either, write `y !== null && y !== undefined`
// expect-error-count: 2
function main(): void {
  const x: number | null | undefined = null;
  const y: string | null | undefined = undefined;
  if (x == (null)) {}
  if ((undefined) != y) {}
}
