// test262: test/built-ins/parseFloat/S15.1.2.3_A4_T2.js

function main(): void {
  assertSameValue(parseFloat("1ex"), 1, 'parseFloat("1ex") must return 1');
  assertSameValue(parseFloat("1e-x"), 1, 'parseFloat("1e-x") must return 1');
  assertSameValue(parseFloat("1e1x"), 10, 'parseFloat("1e1x") must return 10');
  assertSameValue(parseFloat("1e-1x"), 0.1, 'parseFloat("1e-1x") must return 0.1');
  assertSameValue(parseFloat("0.1e-1x"), 0.01, 'parseFloat("0.1e-1x") must return 0.01');
}
