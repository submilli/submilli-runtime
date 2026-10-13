// expect-error: expected `{}`, got `null`
// expect-error: expected `{}`, got `undefined`
// expect-error: expected `{}`, got `unknown`
// expect-error-count: 3

function main(): void {
  const a: {} = null;
  const b: {} = undefined;
  const value: unknown = 1;
  const c: {} = value;
}
