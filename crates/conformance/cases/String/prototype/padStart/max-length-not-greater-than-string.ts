// test262: test/built-ins/String/prototype/padStart/max-length-not-greater-than-string.js
// Numeric rows only; the `undefined`/`null` maxLength rows are rejected by
// the type system.

function main(): void {
  assertSameValue("abc".padStart(NaN, "def"), "abc");
  assertSameValue("abc".padStart(-Infinity, "def"), "abc");
  assertSameValue("abc".padStart(0, "def"), "abc");
  assertSameValue("abc".padStart(-1, "def"), "abc");
  assertSameValue("abc".padStart(3, "def"), "abc");
  assertSameValue("abc".padStart(3.9999, "def"), "abc");
}
