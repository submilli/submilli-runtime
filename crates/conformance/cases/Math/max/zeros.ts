// test262: test/built-ins/Math/max/zeros.js

function main(): void {
  assertSameValue(Math.max(0, 0), 0, "(0, 0)");
  assertSameValue(Math.max(-0, -0), -0, "(-0, -0)");
  assertSameValue(Math.max(0, -0), 0, "(0, -0)");
  assertSameValue(Math.max(-0, 0), 0, "(-0, 0)");
  assertSameValue(Math.max(0, 0, -0), 0, "(0, 0, -0)");
}
