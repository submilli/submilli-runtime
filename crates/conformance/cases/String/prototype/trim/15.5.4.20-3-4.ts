// test262: test/built-ins/String/prototype/trim/15.5.4.20-3-4.js

function main(): void {
  const lineTerminatorsStr = "\u000A\u000D\u2028\u2029";
  const whiteSpacesStr = "\u0009\u000A\u000B\u000C\u000D\u0020\u00A0\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200A\u2028\u2029\u202F\u205F\u3000\uFEFF";
  const str = whiteSpacesStr + lineTerminatorsStr + "abc";

  assertSameValue(str.trim(), "abc", "str.trim()");
}
