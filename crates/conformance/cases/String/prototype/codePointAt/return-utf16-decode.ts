// test262: test/built-ins/String/prototype/codePointAt/return-utf16-decode.js
// Surrogate pairs are built with fromCharCode — the lexer rejects
// unpaired surrogate escapes and these pairs are the test's subject.

function main(): void {
  assertSameValue(String.fromCharCode(0xd800, 0xdc00).codePointAt(0), 65536, "U+10000");
  assertSameValue(String.fromCharCode(0xd800, 0xddd0).codePointAt(0), 66000, "U+101D0");
  assertSameValue(String.fromCharCode(0xd800, 0xdfff).codePointAt(0), 66559, "U+103FF");

  assertSameValue(String.fromCharCode(0xdaaa, 0xdc00).codePointAt(0), 763904, "U+BA800");
  assertSameValue(String.fromCharCode(0xdaaa, 0xddd0).codePointAt(0), 764368, "U+BA9D0");
  assertSameValue(String.fromCharCode(0xdaaa, 0xdfff).codePointAt(0), 764927, "U+BABFF");

  assertSameValue(String.fromCharCode(0xdbff, 0xdc00).codePointAt(0), 1113088, "U+10FC00");
  assertSameValue(String.fromCharCode(0xdbff, 0xddd0).codePointAt(0), 1113552, "U+10FDD0");
  assertSameValue(String.fromCharCode(0xdbff, 0xdfff).codePointAt(0), 1114111, "U+10FFFF");
}
