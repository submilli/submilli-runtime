// test262: test/built-ins/String/prototype/codePointAt/return-first-code-unit.js
// Lone surrogates are built with fromCharCode — the lexer rejects
// unpaired surrogate escapes.

function main(): void {
  assertSameValue(String.fromCharCode(0xd800, 0xdbff).codePointAt(0), 0xd800);
  assertSameValue(String.fromCharCode(0xd800, 0xe000).codePointAt(0), 0xd800);

  assertSameValue(String.fromCharCode(0xdaaa, 0xdbff).codePointAt(0), 0xdaaa);
  assertSameValue(String.fromCharCode(0xdaaa, 0xe000).codePointAt(0), 0xdaaa);

  assertSameValue(String.fromCharCode(0xdbff, 0xdbff).codePointAt(0), 0xdbff);
  assertSameValue(String.fromCharCode(0xdbff, 0xe000).codePointAt(0), 0xdbff);

  assertSameValue(String.fromCharCode(0xd800, 0x0000).codePointAt(0), 0xd800);
  assertSameValue(String.fromCharCode(0xd800, 0xffff).codePointAt(0), 0xd800);

  assertSameValue(String.fromCharCode(0xdaaa, 0x0000).codePointAt(0), 0xdaaa);
  assertSameValue(String.fromCharCode(0xdaaa, 0xffff).codePointAt(0), 0xdaaa);

  assertSameValue(String.fromCharCode(0xdbff, 0xdbff).codePointAt(0), 0xdbff);
  assertSameValue(String.fromCharCode(0xdbff, 0xffff).codePointAt(0), 0xdbff);
}
