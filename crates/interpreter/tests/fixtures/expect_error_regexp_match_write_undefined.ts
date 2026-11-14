// An element of a match array is written as a `string`, as tsc types it.
// expect-error: expected `string`, got `undefined`
function main(): void {
  const m = "ab".match(/(a)/)!;
  m[1] = undefined;
}
