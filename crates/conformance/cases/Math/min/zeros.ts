// test262: test/built-ins/Math/min/zeros.js

function main(): void {
  assertSameValue(Math.min(0, 0), 0, "(0, 0)");
  assertSameValue(Math.min(-0, -0), -0, "(-0, -0)");
  assertSameValue(Math.min(0, -0), -0, "(0, -0)");
  assertSameValue(Math.min(-0, 0), -0, "(-0, 0)");
  assertSameValue(Math.min(0, 0, -0), -0, "(0, 0, -0)");
}
