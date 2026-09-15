// test262: test/built-ins/String/prototype/padStart/normal-operation.js
// The surrogate-pair expected value is built with fromCharCode — the lexer
// rejects the original's trailing lone-surrogate literal.

function main(): void {
  assertSameValue("abc".padStart(7, "def"), "defdabc");
  assertSameValue("abc".padStart(5, "*"), "**abc");

  // surrogate pairs
  assertSameValue(
    "abc".padStart(6, String.fromCharCode(0xd83d, 0xdca9)),
    String.fromCharCode(0xd83d, 0xdca9, 0xd83d) + "abc",
  );
}
