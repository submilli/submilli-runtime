// test262: test/built-ins/Math/clz32/nan.js

function main(): void {
  assertSameValue(Math.clz32(NaN), 32);
}
