// test262: test/built-ins/Math/log2/log2-basicTests.js
// The null/undefined coercion rows are dropped: no ToNumber coercion.

function main(): void {
  assertSameValue(Math.log2(-0), Number.NEGATIVE_INFINITY,
    "Math.log2 produces incorrect output for -0");
  assertSameValue(Math.log2(0), Number.NEGATIVE_INFINITY,
    "Math.log2 produces incorrect output for +0");
  assertSameValue(Math.log2(-0.9), NaN,
    "Math.log2 produces incorrect output for -0.9");
  assertSameValue(Math.log2(NaN), NaN,
    "Math.log2 produces incorrect output for NaN");
  assertSameValue(Math.log2(-10), NaN,
    "Math.log2 produces incorrect output for -10");
  assertSameValue(Math.log2(-Infinity), NaN,
    "Math.log2 produces incorrect output for -Infinity");
  assertSameValue(Math.log2(Number.POSITIVE_INFINITY), Number.POSITIVE_INFINITY,
    "Math.log2 produces incorrect output for Number.POSITIVE_INFINITY");
  assertSameValue(Math.log2(1), 0,
    "Math.log2 produces incorrect output for 1");
  assertSameValue(Math.log2(2.00), 1,
    "Math.log2 produces incorrect output for 2.00");
  assertSameValue(Math.log2(4.00), 2,
    "Math.log2 produces incorrect output for 4.00");
  assertSameValue(Math.log2(8.00), 3,
    "Math.log2 produces incorrect output for 8.00");
}
