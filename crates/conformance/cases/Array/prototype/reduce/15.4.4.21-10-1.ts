// test262: test/built-ins/Array/prototype/reduce/15.4.4.21-10-1.js
// Adapted: reduce requires an explicit initial value here; the point — the
// fold does not mutate the source array — is unchanged.

function main(): void {
  const callbackfn = (prevVal: number, curVal: number): number => 1;

  const srcArr = [1, 2, 3, 4, 5];
  srcArr.reduce(callbackfn, 0);

  assertSameValue(srcArr[0], 1, "srcArr[0]");
  assertSameValue(srcArr[1], 2, "srcArr[1]");
  assertSameValue(srcArr[2], 3, "srcArr[2]");
  assertSameValue(srcArr[3], 4, "srcArr[3]");
  assertSameValue(srcArr[4], 5, "srcArr[4]");
}
