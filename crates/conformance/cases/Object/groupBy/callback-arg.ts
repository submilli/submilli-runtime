// test262: test/built-ins/Object/groupBy/callback-arg.js
// Adaptation: a rest parameter represents arguments, preserving its exact count.
function main(): void {
  const arr = [-0, 0, 1, 2, 3];
  let calls = 0;
  Object.groupBy(arr, (...args: unknown[]): null => {
    calls++;
    const n = args[0] as number;
    const i = args[1] as number;
    assertSameValue(n, arr[i], "selected element aligns with index");
    assertSameValue(args.length, 2, "only two arguments are passed");
    return null;
  });
  assertSameValue(calls, 5, "called for all 5 elements");
}
