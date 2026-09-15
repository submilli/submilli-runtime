// test262: test/built-ins/String/prototype/padEnd/normal-operation.js
// The surrogate-pair expected value is built with fromCharCode — the lexer
// rejects the original's trailing lone-surrogate literal.

function main(): void {
  assertSameValue("abc".padEnd(7, "def"), "abcdefd");
  assertSameValue("abc".padEnd(5, "*"), "abc**");

  // surrogate pairs
  assertSameValue(
    "abc".padEnd(6, String.fromCharCode(0xd83d, 0xdca9)),
    "abc" + String.fromCharCode(0xd83d, 0xdca9, 0xd83d),
  );
}
