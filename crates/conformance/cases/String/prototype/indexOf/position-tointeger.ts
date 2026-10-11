// test262: test/built-ins/String/prototype/indexOf/position-tointeger.js
// Numeric and undefined rows only; the null/string/boolean/array/object
// position rows are rejected by the type system (no implicit ToInteger
// coercion).

function main(): void {
  assertSameValue("aaaa".indexOf("aa", 0), 0);
  assertSameValue("aaaa".indexOf("aa", 1), 1);
  assertSameValue("aaaa".indexOf("aa", -0.9), 0, "ToInteger: truncate towards 0");
  assertSameValue("aaaa".indexOf("aa", 0.9), 0, "ToInteger: truncate towards 0");
  assertSameValue("aaaa".indexOf("aa", 1.9), 1, "ToInteger: truncate towards 0");
  assertSameValue("aaaa".indexOf("aa", NaN), 0, "ToInteger: NaN => 0");
  assertSameValue("aaaa".indexOf("aa", Infinity), -1);
  assertSameValue("aaaa".indexOf("aa", undefined), 0, "ToInteger: undefined => NaN => 0");
  assertSameValue("aaaa".indexOf("aa", 2), 2);
  assertSameValue("aaaa".indexOf("aa", 2.9), 2, "ToInteger: truncate towards 0");
}
