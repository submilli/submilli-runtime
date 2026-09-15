// test262: test/built-ins/Math/tanh/tanh-specialVals.js

function main(): void {
  assertSameValue(Math.tanh(NaN), Number.NaN,
    "Math.tanh produces incorrect output for NaN");
  assertSameValue(Math.tanh(Number.NEGATIVE_INFINITY), -1,
    "Math.tanh should produce -1 for Number.NEGATIVE_INFINITY");
  assertSameValue(Math.tanh(Number.POSITIVE_INFINITY), 1,
    "Math.tanh should produce 1 for Number.POSITIVE_INFINITY");
  assertSameValue(1 / Math.tanh(-0), Number.NEGATIVE_INFINITY,
    "Math.tanh should produce -0 for -0");
  assertSameValue(1 / Math.tanh(0), Number.POSITIVE_INFINITY,
    "Math.tanh should produce +0 for +0");
}
