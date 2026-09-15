// test262: test/built-ins/Number/string-hex-literal-invalid.js

function main(): void {
  assertSameValue(Number("0xG"), NaN, "invalid digit");
  assertSameValue(Number("00x0"), NaN, "leading zero");
  assertSameValue(Number("0x"), NaN, "omitted digits");
  assertSameValue(Number("+0x10"), NaN, "plus sign");
  assertSameValue(Number("-0x10"), NaN, "minus sign");
  assertSameValue(Number("0x10.01"), NaN, "fractional part");
  assertSameValue(Number("0x1e-10"), NaN, "exponent part with a minus sign");
  assertSameValue(Number("0x1e+10"), NaN, "exponent part with a plus sign");
}
