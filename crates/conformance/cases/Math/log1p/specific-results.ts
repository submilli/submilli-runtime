// test262: test/built-ins/Math/log1p/specific-results.js

function main(): void {
  assertSameValue(Math.log1p(NaN), NaN, "NaN");
  assertSameValue(Math.log1p(-1.000001), NaN, "-1.000001");
  assertSameValue(Math.log1p(-2), NaN, "-2");
  assertSameValue(Math.log1p(-Infinity), NaN, "-Infinity");
  assertSameValue(Math.log1p(-1), -Infinity, "-1");
  assertSameValue(Math.log1p(0), 0, "0");
  assertSameValue(Math.log1p(-0), -0, "-0");
  assertSameValue(Math.log1p(Infinity), Infinity, "Infinity");
}
