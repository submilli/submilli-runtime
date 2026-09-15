// test262: test/built-ins/Math/sqrt/S15.8.2.17_A2.js

function main(): void {
  assertSameValue(Math.sqrt(-0.000000000000001), NaN, "-0.000000000000001");
  assertSameValue(Math.sqrt(-1), NaN, "-1");
  assertSameValue(Math.sqrt(-Infinity), NaN, "-Infinity");
}
