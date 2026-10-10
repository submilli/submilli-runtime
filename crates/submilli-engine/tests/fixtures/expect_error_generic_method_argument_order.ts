// A builtin generic method checks its arguments in order too: `mk(s)` runs
// before the next argument assigns `s`.
// expect-error: expected `string`, got `null`
// expect-error-count: 1
function mk(s: string): (a: number, b: number) => number {
  return (a, b) => a + b + s.length;
}
function main(): void {
  let s: string | null = null;
  console.log([1, 2].reduce(mk(s), (s = "xyz").length));
}
