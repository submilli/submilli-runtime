// test262: test/built-ins/Number/isFinite/finite-numbers.js

function main(): void {
  assertSameValue(Number.isFinite(-10), true, "-10");
  assertSameValue(Number.isFinite(-0), true, "-0");
  assertSameValue(Number.isFinite(0), true, "0");
  assertSameValue(Number.isFinite(10), true, "10");
  assertSameValue(Number.isFinite(1e10), true, "1e10");
  assertSameValue(Number.isFinite(10.10), true, "10.10");
  assertSameValue(Number.isFinite(9007199254740991), true, "9007199254740991");
  assertSameValue(Number.isFinite(-9007199254740991), true, "-9007199254740991");
  assertSameValue(Number.isFinite(Number.MAX_VALUE), true, "Number.MAX_VALUE");
}
