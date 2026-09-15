// test262: test/built-ins/Math/cbrt/cbrt-specialValues.js

function main(): void {
  assertSameValue(Math.cbrt(NaN), Number.NaN,
    "Math.cbrt produces incorrect output for NaN");
  assertSameValue(Math.cbrt(Number.NEGATIVE_INFINITY), Number.NEGATIVE_INFINITY,
    "Math.cbrt should produce negative infinity for Number.NEGATIVE_INFINITY");
  assertSameValue(Math.cbrt(Number.POSITIVE_INFINITY), Number.POSITIVE_INFINITY,
    "Math.cbrt should produce positive infinity for Number.POSITIVE_INFINITY");
  assertSameValue(1 / Math.cbrt(-0), Number.NEGATIVE_INFINITY,
    "Math.cbrt should produce -0 for -0");
  assertSameValue(1 / Math.cbrt(0), Number.POSITIVE_INFINITY,
    "Math.cbrt should produce +0 for +0");
}
