// Testing for either names the operand twice, so an operand that may have
// effects is bound to a `const` first. The strict form evaluates it once.
// expect-error: `next() != undefined` does not also match `null`
// expect-error: to test for either, first bind `next()` to a `const` (`const value = next();`), then write `value !== null && value !== undefined`
// expect-error: to test for `undefined` alone, write `next() !== undefined`
// expect-error: `box.value == null` does not also match `undefined`
// expect-error: to test for either, write `box.value === null || box.value === undefined`
// expect-error-count: 2
function next(): number | null | undefined {
  return 1;
}

function main(): void {
  const box: { value: number | null | undefined } = { value: null };
  if (next() != undefined) {}
  if (box.value == null) {}
}
