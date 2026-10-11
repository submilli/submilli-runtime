// A left side of `??` that doesn't fit the context is reported once, for the
// whole expression, as tsc does.
// expect-error: expected `string`, got `5 | "d"`
// expect-error-count: 1
function main(): void {
  const flag = Math.random() < 2;
  const s: string = (flag ? 5 : undefined) ?? "d";
}
