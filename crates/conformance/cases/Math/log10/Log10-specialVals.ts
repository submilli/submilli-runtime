// test262: test/built-ins/Math/log10/Log10-specialVals.js
// The null/undefined coercion rows are dropped: no ToNumber coercion.

function main(): void {
  assertSameValue(Math.log10(-0), Number.NEGATIVE_INFINITY,
    "Math.log10 produces incorrect output for -0");
  assertSameValue(Math.log10(0), Number.NEGATIVE_INFINITY,
    "Math.log10 produces incorrect output for +0");
  assertSameValue(Math.log10(-0.9), Number.NaN,
    "Math.log10 produces incorrect output for -0.9");
  assertSameValue(Math.log10(NaN), Number.NaN,
    "Math.log10 produces incorrect output for NaN");
  assertSameValue(Math.log10(-10), Number.NaN,
    "Math.log10 produces incorrect output for -10");
  assertSameValue(Math.log10(Number.POSITIVE_INFINITY), Number.POSITIVE_INFINITY,
    "Math.log10 produces incorrect output for Number.POSITIVE_INFINITY");
  assertSameValue(Math.log10(1), 0,
    "Math.log10 produces incorrect output for 1");
  assertSameValue(Math.log10(10.00), 1,
    "Math.log10 produces incorrect output for 10.00");
  assertSameValue(Math.log10(100.00), 2,
    "Math.log10 produces incorrect output for 100.00");
  assertSameValue(Math.log10(1000.00), 3,
    "Math.log10 produces incorrect output for 1000.00");
}
