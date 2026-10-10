// test262: test/built-ins/Number/prototype/toExponential/undefined-fractiondigits.js

function main(): void {
  assertSameValue((123.456).toExponential(undefined), "1.23456e+2", "undefined");
  assertSameValue((123.456).toExponential(), "1.23456e+2", "no arg");
  assertSameValue((123.456).toExponential(0), "1e+2", "0");

  assertSameValue((1.1e-32).toExponential(undefined), "1.1e-32", "undefined");
  assertSameValue((1.1e-32).toExponential(), "1.1e-32", "no arg");
  assertSameValue((1.1e-32).toExponential(0), "1e-32", "0");

  assertSameValue((100).toExponential(undefined), "1e+2", "undefined");
  assertSameValue((100).toExponential(), "1e+2", "no arg");
  assertSameValue((100).toExponential(0), "1e+2", "0");
}
