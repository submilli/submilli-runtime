// test262: test/built-ins/Number/S9.3.1_A3_T1.js

const ws: string = "\u0009\u000C\u0020\u00A0\u000B\u000A\u000D\u2028\u2029\u1680\u2000\u2001\u2002\u2003\u2004\u2005\u2006\u2007\u2008\u2009\u200A\u202F\u205F\u3000";

function main(): void {
  assertSameValue(Number(ws), 0);
  assertSameValue(Number(ws + "1234567890" + ws), 1234567890);
  assertSameValue(Number(ws + "Infinity" + ws), Infinity);
  assertSameValue(Number(ws + "-Infinity" + ws), -Infinity);
}
