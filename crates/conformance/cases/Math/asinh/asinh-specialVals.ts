// test262: test/built-ins/Math/asinh/asinh-specialVals.js

function main(): void {
  assertSameValue(Math.asinh(NaN), Number.NaN,
    "Math.asinh produces incorrect output for NaN");
  assertSameValue(Math.asinh(Number.NEGATIVE_INFINITY), Number.NEGATIVE_INFINITY,
    "Math.asinh should produce negative infinity for Number.NEGATIVE_INFINITY");
  assertSameValue(Math.asinh(Number.POSITIVE_INFINITY), Number.POSITIVE_INFINITY,
    "Math.asinh should produce positive infinity for Number.POSITIVE_INFINITY");
  assertSameValue(1 / Math.asinh(-0), Number.NEGATIVE_INFINITY,
    "Math.asinh should produce -0 for -0");
  assertSameValue(1 / Math.asinh(0), Number.POSITIVE_INFINITY,
    "Math.asinh should produce +0 for +0");
}
