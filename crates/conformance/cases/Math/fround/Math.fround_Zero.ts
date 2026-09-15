// test262: test/built-ins/Math/fround/Math.fround_Zero.js

function main(): void {
  assertSameValue(Math.fround(0), 0, "Math.fround(0)");
  assertSameValue(Math.fround(-0), -0, "Math.fround(-0)");
}
