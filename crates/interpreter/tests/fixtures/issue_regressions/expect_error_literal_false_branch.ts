// expect-error: expected `string`, got `unknown`
function partial(x: unknown): string {
  if (x !== "a") { return x; }
  return "";
}
function main(): void {}
