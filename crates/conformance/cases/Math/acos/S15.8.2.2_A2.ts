// test262: test/built-ins/Math/acos/S15.8.2.2_A2.js

function main(): void {
  assertSameValue(Math.acos(1.000000000000001), NaN, "1.000000000000001");
  assertSameValue(Math.acos(2), NaN, "2");
  assertSameValue(Math.acos(Infinity), NaN, "Infinity");
}
