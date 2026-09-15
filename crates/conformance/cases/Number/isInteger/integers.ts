// test262: test/built-ins/Number/isInteger/integers.js

function main(): void {
  assertSameValue(Number.isInteger(478), true, "Number.isInteger(478)");
  assertSameValue(Number.isInteger(-0), true, "-0");
  assertSameValue(Number.isInteger(0), true, "0");
  assertSameValue(Number.isInteger(-1), true, "-1");
  assertSameValue(Number.isInteger(9007199254740991), true, "9007199254740991");
  assertSameValue(Number.isInteger(-9007199254740991), true, "-9007199254740991");
  assertSameValue(Number.isInteger(9007199254740992), true, "9007199254740992");
  assertSameValue(Number.isInteger(-9007199254740992), true, "-9007199254740992");
}
