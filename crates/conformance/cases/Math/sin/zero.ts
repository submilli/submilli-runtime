// test262: test/built-ins/Math/sin/zero.js

function main(): void {
  assertSameValue(Math.sin(0), 0, "+0");
  assertSameValue(Math.sin(-0), -0, "-0");
}
