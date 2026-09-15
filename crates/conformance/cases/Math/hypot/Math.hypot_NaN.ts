// test262: test/built-ins/Math/hypot/Math.hypot_NaN.js

function main(): void {
  assertSameValue(Math.hypot(NaN, 3), NaN);
}
