// test262: test/built-ins/String/prototype/trim/15.5.4.20-3-2.js
// expect-fail: trim should strip U+FEFF (ZWNBSP is ECMA WhiteSpace); the Unicode White_Space set used by the host leaves it in place

function main(): void {
  const whiteSpacesStr = "\u0009\u000A\u000B\u000C\u000D\u0020\u00A0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200A\u2028\u2029\u202F\u205F\u3000\uFEFF";

  assertSameValue(whiteSpacesStr.trim(), "", "whiteSpacesStr.trim()");
}
