// test262: test/built-ins/Number/S9.3.1_A2.js

const ws: string = "\u0009\u000C\u0020\u00A0\u000B\u000A\u000D\u2028\u2029\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200A\u202F\u205F\u3000";

function main(): void {
  assertSameValue(Number(ws), 0, "Number(<all StrWhiteSpace chars>) must return 0");

  assertSameValue(Number(" "), 0, "Number(u0020) must return 0");
  assertSameValue(Number("\t"), 0, "Number(tab) must return 0");
  assertSameValue(Number("\r"), 0, "Number(CR) must return 0");
  assertSameValue(Number("\n"), 0, "Number(LF) must return 0");
  assertSameValue(Number("\f"), 0, "Number(FF) must return 0");
  assertSameValue(Number("\u0009"), 0, "Number(u0009) must return 0");
  assertSameValue(Number("\u000C"), 0, "Number(u000C) must return 0");
  assertSameValue(Number("\u0020"), 0, "Number(u0020) must return 0");
  assertSameValue(Number("\u00A0"), 0, "Number(u00A0) must return 0");
  assertSameValue(Number("\u000B"), 0, "Number(u000B) must return 0");
  assertSameValue(Number("\u000A"), 0, "Number(u000A) must return 0");
  assertSameValue(Number("\u000D"), 0, "Number(u000D) must return 0");
  assertSameValue(Number("\u2028"), 0, "Number(u2028) must return 0");
  assertSameValue(Number("\u2029"), 0, "Number(u2029) must return 0");
  assertSameValue(Number("\u1680"), 0, "Number(u1680) must return 0");
  assertSameValue(Number("\u2000"), 0, "Number(u2000) must return 0");
  assertSameValue(Number("\u2001"), 0, "Number(u2001) must return 0");
  assertSameValue(Number("\u2002"), 0, "Number(u2002) must return 0");
  assertSameValue(Number("\u2003"), 0, "Number(u2003) must return 0");
  assertSameValue(Number("\u2004"), 0, "Number(u2004) must return 0");
  assertSameValue(Number("\u2005"), 0, "Number(u2005) must return 0");
  assertSameValue(Number("\u2006"), 0, "Number(u2006) must return 0");
  assertSameValue(Number("\u2007"), 0, "Number(u2007) must return 0");
  assertSameValue(Number("\u2008"), 0, "Number(u2008) must return 0");
  assertSameValue(Number("\u2009"), 0, "Number(u2009) must return 0");
  assertSameValue(Number("\u200A"), 0, "Number(u200A) must return 0");
  assertSameValue(Number("\u202F"), 0, "Number(u202F) must return 0");
  assertSameValue(Number("\u205F"), 0, "Number(u205F) must return 0");
  assertSameValue(Number("\u3000"), 0, "Number(u3000) must return 0");
}
