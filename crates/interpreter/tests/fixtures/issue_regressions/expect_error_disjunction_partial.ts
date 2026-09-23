// expect-error: expected `string`, got `unknown`
function partial(x: unknown, allow: boolean): string {
  if (typeof x === "string" || allow) { return x; }
  return "";
}
function main(): void {}
