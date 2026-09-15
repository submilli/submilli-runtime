// test262: test/built-ins/parseFloat/S15.1.2.3_A5_T1.js

function main(): void {
  assertSameValue(parseFloat("Infinity"), Number.POSITIVE_INFINITY, 'parseFloat("Infinity") must return Number.POSITIVE_INFINITY');
  assertSameValue(parseFloat("+Infinity"), Number.POSITIVE_INFINITY, 'parseFloat("+Infinity") must return Number.POSITIVE_INFINITY');
  assertSameValue(parseFloat("-Infinity"), Number.NEGATIVE_INFINITY, 'parseFloat("-Infinity") must return Number.NEGATIVE_INFINITY');
}
