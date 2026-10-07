// After arithmetic written back, a literal-typed variable reads as the base
// type, so it no longer fits the literal; a plain `=` of a computed number
// is still checked against the literal.
// expect-error: expected `1`, got `number`
// expect-error-count: 2
function main(): void {
  let z: 1 = 1;
  z += 1;
  const one: 1 = z;
  const r = z + 1;
  let y: 1 = 1;
  y = r;
}
