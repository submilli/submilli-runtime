// test262: test/built-ins/Math/atanh/atanh-specialVals.js

function main(): void {
  assertSameValue(Math.atanh(-1.9), Number.NaN,
    "Math.atanh produces incorrect output for -1.9");
  assertSameValue(Math.atanh(NaN), Number.NaN,
    "Math.atanh produces incorrect output for NaN");
  assertSameValue(Math.atanh(-10), Number.NaN,
    "Math.atanh produces incorrect output for -10");
  assertSameValue(Math.atanh(-Infinity), Number.NaN,
    "Math.atanh produces incorrect output for -Infinity");
  assertSameValue(Math.atanh(1.9), Number.NaN,
    "Math.atanh produces incorrect output for 1.9");
  assertSameValue(Math.atanh(10), Number.NaN,
    "Math.atanh produces incorrect output for 10");
  assertSameValue(Math.atanh(Number.POSITIVE_INFINITY), Number.NaN,
    "Math.atanh produces incorrect output for Number.POSITIVE_INFINITY");

  assertSameValue(Math.atanh(-1), Number.NEGATIVE_INFINITY,
    "Math.atanh should produce negative infinity for -1");
  assertSameValue(Math.atanh(1), Number.POSITIVE_INFINITY,
    "Math.atanh should produce positive infinity for +1");
  assertSameValue(1 / Math.atanh(-0), Number.NEGATIVE_INFINITY,
    "Math.atanh should produce -0 for -0");
  assertSameValue(1 / Math.atanh(0), Number.POSITIVE_INFINITY,
    "Math.atanh should produce +0 for +0");
}
