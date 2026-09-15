// test262: test/built-ins/Math/hypot/Math.hypot_InfinityNaN.js

function main(): void {
  assertSameValue(Math.hypot(NaN, Infinity), Infinity, "Math.hypot(NaN, Infinity)");
}
