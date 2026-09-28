// A callback whose result doesn't fit is reported once, at its body.
// expect-error: expected `string`, got `number`
// expect-error-count: 1
function main(): void {
  const xs = [1, 2];
  const r: string[] = xs.map((v) => v);
}
