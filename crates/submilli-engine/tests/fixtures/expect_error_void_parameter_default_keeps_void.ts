// expect-error: `+` not defined for `number | void` and `number`
// expect-error-count: 1
// A default replaces only an actual `undefined`. A `void` member stays in the
// parameter's type, since a `void` result may hold a value, as tsc reports
// (TS2365).
function f(x: number | void = 5): number {
  return x + 1;
}
function main(): void {
  console.log(f());
}
