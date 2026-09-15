// test262: test/built-ins/Number/MIN_VALUE/value.js

function main(): void {
  assert(Number.MIN_VALUE > 0, "Number.MIN_VALUE must be positive");

  assert(Number.MIN_VALUE < Number.EPSILON, "Number.MIN_VALUE should be smaller than Number.EPSILON");

  assertSameValue(Number.MIN_VALUE / 2, 0, "Number.MIN_VALUE divided by 2 should underflow to 0");
}
