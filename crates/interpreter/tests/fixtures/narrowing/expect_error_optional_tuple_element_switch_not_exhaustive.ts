// expect-error: function `f` does not return a value on all paths
// expect-error-count: 1
// An optional tuple element may be absent, so cases for its values leave
// `undefined` unmatched.
type T = [string, null?] | [number, boolean];
function f(t: T): number {
  switch (t[1]) {
    case null:
      return 1;
    case true:
      return 2;
    case false:
      return 3;
  }
}
function main(): void {
  assert(f([1, true]) === 2);
}
