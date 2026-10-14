// expect-error: destructuring pattern has 2 elements, but right-hand side tuple has 1 element
// expect-error-count: 1
// A parameter's array literal default is a tuple, so a pattern slot past its
// end without a default reads nothing, as tsc reports.
function f([x, y] = [1]): number {
  return x + y;
}
function main(): void {
  f();
}
