// test262: test/built-ins/String/prototype/trimEnd/this-value-whitespace.js
// The original's `\xHH` escapes are spelled `\u00HH`; the prototype
// `.call(str)` is a plain method call.

function main(): void {
  // A string of all valid WhiteSpace Unicode code points
  const wspc = "\u0009\u000A\u000B\u000C\u000D\u0020\u00A0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200A\u202F\u205F\u3000\u2028\u2029\uFEFF";

  const str = wspc + "a" + wspc + "b" + wspc;
  const expected = wspc + "a" + wspc + "b";

  assertSameValue(str.trimEnd(), expected);
}
