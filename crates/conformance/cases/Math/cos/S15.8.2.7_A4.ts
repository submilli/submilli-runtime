// test262: test/built-ins/Math/cos/S15.8.2.7_A4.js

function main(): void {
  assertSameValue(Math.cos(Infinity), NaN);
}
