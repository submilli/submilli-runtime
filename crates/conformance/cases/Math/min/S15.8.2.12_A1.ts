// test262: test/built-ins/Math/min/S15.8.2.12_A1.js

function main(): void {
  assertSameValue(Math.min(), Infinity, "Math.min() must return +Infinity");
}
