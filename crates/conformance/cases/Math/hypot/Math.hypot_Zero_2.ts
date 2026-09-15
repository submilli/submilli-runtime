// test262: test/built-ins/Math/hypot/Math.hypot_Zero_2.js

function main(): void {
  assertSameValue(Math.hypot(0), 0, "Math.hypot(0)");
  assertSameValue(Math.hypot(-0), 0, "Math.hypot(-0)");
  assertSameValue(Math.hypot(0, 0), 0, "Math.hypot(0, 0)");
  assertSameValue(Math.hypot(0, -0), 0, "Math.hypot(0, -0)");
  assertSameValue(Math.hypot(-0, 0), 0, "Math.hypot(-0, 0)");
  assertSameValue(Math.hypot(-0, -0), 0, "Math.hypot(-0, -0)");
  assertSameValue(Math.hypot(0, -0, -0), 0, "Math.hypot(0, -0, -0)");
}
