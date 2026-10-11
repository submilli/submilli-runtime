// test262: test/built-ins/String/fromCharCode/S9.7_A1.js
// Adapted: each `!== +0` / `1/x !== +Infinity` pair is one sign check.

function main(): void {
  assertSameValue(1 / String.fromCharCode(Number.NaN).charCodeAt(0), Infinity, "NaN converts to +0");
  assertSameValue(1 / String.fromCharCode(0).charCodeAt(0), Infinity, "+0 converts to +0");
  assertSameValue(1 / String.fromCharCode(-0).charCodeAt(0), Infinity, "-0 converts to +0");
  assertSameValue(1 / String.fromCharCode(Number.POSITIVE_INFINITY).charCodeAt(0), Infinity, "+Infinity converts to +0");
  assertSameValue(1 / String.fromCharCode(Number.NEGATIVE_INFINITY).charCodeAt(0), Infinity, "-Infinity converts to +0");
}
