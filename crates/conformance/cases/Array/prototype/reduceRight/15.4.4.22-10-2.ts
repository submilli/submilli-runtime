// test262: test/built-ins/Array/prototype/reduceRight/15.4.4.22-10-2.js
// Adapted: reduceRight requires an explicit initial value here, so "" stands
// in for the absent-initial form; the right-to-left fold result is unchanged.

function main(): void {
  const callbackfn = (prevVal: string, curVal: string): string => prevVal + curVal;

  const srcArr = ["1", "2", "3", "4", "5"];

  const result = srcArr.reduceRight(callbackfn, "");
  assertSameValue(result, "54321", "srcArr.reduceRight(callbackfn)");
}
