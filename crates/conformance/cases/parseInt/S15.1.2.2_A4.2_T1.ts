// test262: test/built-ins/parseInt/S15.1.2.2_A4.2_T1.js

function main(): void {
  assertSameValue(parseInt("0", 1), NaN, 'parseInt("0", 1) must return NaN');
  assertSameValue(parseInt("1", 1), NaN, 'parseInt("1", 1) must return NaN');
  assertSameValue(parseInt("2", 1), NaN, 'parseInt("2", 1) must return NaN');
  assertSameValue(parseInt("3", 1), NaN, 'parseInt("3", 1) must return NaN');
  assertSameValue(parseInt("4", 1), NaN, 'parseInt("4", 1) must return NaN');
  assertSameValue(parseInt("5", 1), NaN, 'parseInt("5", 1) must return NaN');
  assertSameValue(parseInt("6", 1), NaN, 'parseInt("6", 1) must return NaN');
  assertSameValue(parseInt("7", 1), NaN, 'parseInt("7", 1) must return NaN');
  assertSameValue(parseInt("8", 1), NaN, 'parseInt("8", 1) must return NaN');
  assertSameValue(parseInt("9", 1), NaN, 'parseInt("9", 1) must return NaN');
  assertSameValue(parseInt("10", 1), NaN, 'parseInt("10", 1) must return NaN');
  assertSameValue(parseInt("11", 1), NaN, 'parseInt("11", 1) must return NaN');
}
