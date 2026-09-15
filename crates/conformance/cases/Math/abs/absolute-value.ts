// test262: test/built-ins/Math/abs/absolute-value.js

function main(): void {
  assertSameValue(Math.abs(-42), 42, "-42");
  assertSameValue(Math.abs(42), 42, "42");
  assertSameValue(Math.abs(-0.000001), 0.000001, "-0.000001");
  assertSameValue(Math.abs(0.000001), 0.000001, "0.000001");
  assertSameValue(Math.abs(-1e-17), 1e-17, "-1e-17");
  assertSameValue(Math.abs(1e-17), 1e-17, "1e-17");
  assertSameValue(Math.abs(-9007199254740991), 9007199254740991, "-(2**53-1)");
  assertSameValue(Math.abs(9007199254740991), 9007199254740991, "2**53-1");
}
