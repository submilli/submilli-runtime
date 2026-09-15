// test262: test/built-ins/Number/isSafeInteger/safe-integers.js

function main(): void {
  assertSameValue(Number.isSafeInteger(1), true, "1");
  assertSameValue(Number.isSafeInteger(-0), true, "-0");
  assertSameValue(Number.isSafeInteger(0), true, "0");
  assertSameValue(Number.isSafeInteger(-1), true, "-1");
  assertSameValue(Number.isSafeInteger(9007199254740991), true, "9007199254740991");
  assertSameValue(Number.isSafeInteger(-9007199254740991), true, "-9007199254740991");
}
