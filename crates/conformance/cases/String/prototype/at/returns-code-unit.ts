// test262: test/built-ins/String/prototype/at/returns-code-unit.js
// The lone-surrogate literal "12\uD80034" is built with fromCharCode —
// the lexer rejects unpaired surrogate escapes.

function main(): void {
  const s = "12" + String.fromCharCode(0xd800) + "34";

  assertSameValue(s.at(0), "1", 's.at(0) must return "1"');
  assertSameValue(s.at(1), "2", 's.at(1) must return "2"');
  assertSameValue(s.at(2), String.fromCharCode(0xd800), 's.at(2) must return "\\uD800"');
  assertSameValue(s.at(3), "3", 's.at(3) must return "3"');
  assertSameValue(s.at(4), "4", 's.at(4) must return "4"');
}
