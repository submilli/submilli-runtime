// expect-error: method `reduce` expects 2 argument(s), got 1
// Not a test262 port: pins a documented divergence (spec.md §1.2). reduce
// requires an explicit initial value — JS's absent-initial form (seed with
// the first element, TypeError on empty) is a compile-time arity error here.

function main(): void {
  const arr = [1, 2, 3];
  const sum = arr.reduce((acc: number, x: number): number => acc + x);
  assertSameValue(sum, 6, "sum");
}
