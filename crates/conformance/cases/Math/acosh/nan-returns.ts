// test262: test/built-ins/Math/acosh/nan-returns.js

function main(): void {
  assertSameValue(Math.acosh(NaN), NaN, "NaN");
  assertSameValue(Math.acosh(0.999999), NaN, "0.999999");
  assertSameValue(Math.acosh(0), NaN, "0");
  assertSameValue(Math.acosh(-1), NaN, "-1");
  assertSameValue(Math.acosh(-Infinity), NaN, "-Infinity");
}
