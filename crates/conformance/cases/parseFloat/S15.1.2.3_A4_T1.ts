// test262: test/built-ins/parseFloat/S15.1.2.3_A4_T1.js

function main(): void {
  assertSameValue(parseFloat("0x"), 0, 'parseFloat("0x") must return 0');
  assertSameValue(parseFloat("11x"), 11, 'parseFloat("11x") must return 11');
  assertSameValue(parseFloat("11s1"), 11, 'parseFloat("11s1") must return 11');
  assertSameValue(parseFloat("11.s1"), 11, 'parseFloat("11.s1") must return 11');
  assertSameValue(parseFloat(".0s1"), 0, 'parseFloat(".0s1") must return 0');
}
