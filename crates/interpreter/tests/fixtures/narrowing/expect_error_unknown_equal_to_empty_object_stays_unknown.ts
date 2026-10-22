// expect-error: expected `object`, got `unknown`
// expect-error-count: 1
// `{}` holds primitives too, so an `unknown` equal to one isn't an `object`.
function f(x: unknown, y: {}): string {
  if (x === y) {
    const o: object = x;
    return typeof o;
  }
  return "no";
}
function main(): void {
  assert(f(1, 2) === "no");
}
