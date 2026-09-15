// test262: test/built-ins/Math/max/S15.8.2.11_A1.js

function main(): void {
  assertSameValue(Math.max(), -Infinity, "Math.max() must return -Infinity");
}
