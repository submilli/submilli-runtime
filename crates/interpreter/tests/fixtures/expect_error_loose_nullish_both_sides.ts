// expect-error: `null == undefined` compares two nullish values
// expect-error: in TypeScript this is always `true`; write `true`
// expect-error: `0 != null` does not also match `undefined`
// expect-error: to test for either, write `0 !== null && 0 !== undefined`
// expect-error-count: 2
// Two nullish sides always compare equal under TypeScript's `==`, so the help
// names that value; a literal operand repeats inline instead of being bound to
// a `const` first.
function main(): void {
  if (null == undefined) {}
  if (0 != null) {}
}
