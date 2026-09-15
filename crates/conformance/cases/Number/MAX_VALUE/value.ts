// test262: test/built-ins/Number/MAX_VALUE/value.js

function main(): void {
  assert(Number.MAX_VALUE > 0, "Number.MAX_VALUE must be positive");

  assert(Number.MAX_VALUE > Number.MAX_SAFE_INTEGER, "Number.MAX_VALUE should be larger than Number.MAX_SAFE_INTEGER");
  assert(Number.MAX_VALUE < Infinity, "Number.MAX_VALUE should be less than Infinity");

  assertSameValue(Number.MAX_VALUE * 2, Infinity, "Number.MAX_VALUE multiplied by 2 should be Infinity");
  assertSameValue(Number.MAX_VALUE, 1.7976931348623157e+308, "Number.MAX_VALUE should be approximately 1.7976931348623157e+308");
}
