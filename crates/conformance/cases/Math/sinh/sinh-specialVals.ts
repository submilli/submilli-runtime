// test262: test/built-ins/Math/sinh/sinh-specialVals.js

function main(): void {
  assertSameValue(Math.sinh(NaN), Number.NaN,
    "Math.sinh produces incorrect output for NaN");
  assertSameValue(Math.sinh(Number.NEGATIVE_INFINITY), Number.NEGATIVE_INFINITY,
    "Math.sinh should produce negative infinity for Number.NEGATIVE_INFINITY");
  assertSameValue(Math.sinh(Number.POSITIVE_INFINITY), Number.POSITIVE_INFINITY,
    "Math.sinh should produce positive infinity for Number.POSITIVE_INFINITY");
  assertSameValue(1 / Math.sinh(-0), Number.NEGATIVE_INFINITY,
    "Math.sinh should produce -0 for -0");
  assertSameValue(1 / Math.sinh(0), Number.POSITIVE_INFINITY,
    "Math.sinh should produce +0 for +0");
}
