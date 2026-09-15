// test262: test/built-ins/Array/prototype/reduce/15.4.4.21-10-2.js
// Adapted: reduce requires an explicit initial value here, so "" stands in
// for the absent-initial form; the left-to-right fold result is unchanged.

function main(): void {
  const callbackfn = (prevVal: string, curVal: string): string => prevVal + curVal;

  const srcArr = ["1", "2", "3", "4", "5"];

  const result = srcArr.reduce(callbackfn, "");
  assertSameValue(result, "12345", "srcArr.reduce(callbackfn)");
}
