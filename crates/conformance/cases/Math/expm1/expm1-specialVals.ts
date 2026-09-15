// test262: test/built-ins/Math/expm1/expm1-specialVals.js

function main(): void {
  assertSameValue(Math.expm1(NaN), Number.NaN,
    "Math.expm1 produces incorrect output for NaN");
  assertSameValue(Math.expm1(Number.NEGATIVE_INFINITY), -1,
    "Math.expm1 should produce -1 for Number.NEGATIVE_INFINITY");
  assertSameValue(Math.expm1(Number.POSITIVE_INFINITY), Number.POSITIVE_INFINITY,
    "Math.expm1 should produce POSITIVE infinity for Number.POSITIVE_INFINITY");
  assertSameValue(1 / Math.expm1(-0), Number.NEGATIVE_INFINITY,
    "Math.expm1 should produce -0 for -0");
  assertSameValue(1 / Math.expm1(0), Number.POSITIVE_INFINITY,
    "Math.expm1 should produce +0 for +0");
}
