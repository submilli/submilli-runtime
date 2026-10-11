// A function literal called on the spot has no contextual signature in tsc, so
// the literal it returns widens: the call is a `string`.
// expect-error: expected `"x" | "y"`, got `string`
// expect-error-count: 1
function main(): void {
  const picked: "x" | "y" = (() => "x")();
  console.log(picked);
}
