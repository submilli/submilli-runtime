// test262: test/built-ins/Math/exp/S15.8.2.8_A5.js

function main(): void {
  assertSameValue(Math.exp(-Infinity), 0);
}
