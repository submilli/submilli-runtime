// expect-error: expected `object`, got `unknown`
// expect-error-count: 2
// An interface a primitive is assignable to may hold one, so an `unknown`
// equal to its value is not known to be an object.
interface Empty {}
interface Sized {
  length: number;
}
function f(x: unknown, e: Empty, s: Sized): number {
  if (x === e) {
    const o: object = x;
  }
  if (x === s) {
    const o: object = x;
  }
  return 0;
}
function main(): void {
  f(5, 5, "abc");
}
