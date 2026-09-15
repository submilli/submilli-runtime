// test262: test/built-ins/parseFloat/S15.1.2.3_A3_T1.js

function main(): void {
  assertSameValue(parseFloat("str"), NaN, "str");
  assertSameValue(parseFloat("s1"), NaN, "s1");
  assertSameValue(parseFloat(""), NaN, "");
  assertSameValue(parseFloat("+"), NaN, "+");
}
