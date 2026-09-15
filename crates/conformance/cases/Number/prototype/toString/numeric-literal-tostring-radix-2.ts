// test262: test/built-ins/Number/prototype/toString/numeric-literal-tostring-radix-2.js

function main(): void {
  assertSameValue((0).toString(2), "0");
  assertSameValue((1).toString(2), "1");
  assertSameValue(NaN.toString(2), "NaN");
  assertSameValue(Infinity.toString(2), "Infinity");
}
