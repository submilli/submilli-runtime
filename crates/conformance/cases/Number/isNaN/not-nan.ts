// test262: test/built-ins/Number/isNaN/not-nan.js

function main(): void {
  assertSameValue(Number.isNaN(0), false, "0");
  assertSameValue(Number.isNaN(-0), false, "-0");
  assertSameValue(Number.isNaN(1), false, "1");
  assertSameValue(Number.isNaN(-1), false, "-1");
  assertSameValue(Number.isNaN(1.1), false, "1.1");
  assertSameValue(Number.isNaN(1e10), false, "1e10");
  assertSameValue(Number.isNaN(Infinity), false, "Infinity");
  assertSameValue(Number.isNaN(-Infinity), false, "-Infinity");
}
