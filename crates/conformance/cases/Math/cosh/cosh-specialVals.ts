// test262: test/built-ins/Math/cosh/cosh-specialVals.js

function main(): void {
  assertSameValue(Math.cosh(NaN), Number.NaN,
    "Math.cosh produces incorrect output for NaN");
  assertSameValue(Math.cosh(0), 1, "Math.cosh should produce 1 for input = 0");
  assertSameValue(Math.cosh(-0), 1, "Math.cosh should produce 1 for input = -0");
  assertSameValue(Math.cosh(Number.NEGATIVE_INFINITY), Number.POSITIVE_INFINITY,
    "Math.cosh should produce Number.POSITIVE_INFINITY for Number.NEGATIVE_INFINITY");
  assertSameValue(Math.cosh(Number.POSITIVE_INFINITY), Number.POSITIVE_INFINITY,
    "Math.cosh should produce Number.POSITIVE_INFINITY for Number.POSITIVE_INFINITY");
}
