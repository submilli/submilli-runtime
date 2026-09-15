// test262: test/built-ins/Number/S15.7.1.1_A1.js
//
// The typeof and Number-object arms are not portable (typeof is a narrowing
// guard only; no boxing); the string-grammar arms are the intent kept here.

function main(): void {
  assertSameValue(Number("abc"), NaN, 'Number("abc") returns NaN');
  assertSameValue(Number("INFINITY"), NaN, 'Number("INFINITY") returns NaN');
  assertSameValue(Number("infinity"), NaN, 'Number("infinity") returns NaN');
}
