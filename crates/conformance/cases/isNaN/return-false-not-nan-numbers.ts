// test262: test/built-ins/isNaN/return-false-not-nan-numbers.js

function main(): void {
  assertSameValue(isNaN(0), false, "0");
  assertSameValue(isNaN(-0), false, "-0");
  assertSameValue(isNaN(Math.pow(2, 53)), false, "Math.pow(2, 53)");
  assertSameValue(isNaN(-Math.pow(2, 53)), false, "-Math.pow(2, 53)");
  assertSameValue(isNaN(1), false, "1");
  assertSameValue(isNaN(-1), false, "-1");
  assertSameValue(isNaN(0.000001), false, "0.000001");
  assertSameValue(isNaN(-0.000001), false, "-0.000001");
  assertSameValue(isNaN(1e42), false, "1e42");
  assertSameValue(isNaN(-1e42), false, "-1e42");
  assertSameValue(isNaN(Infinity), false, "Infinity");
  assertSameValue(isNaN(-Infinity), false, "-Infinity");
}
