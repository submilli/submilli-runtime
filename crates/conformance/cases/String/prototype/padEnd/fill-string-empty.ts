// test262: test/built-ins/String/prototype/padEnd/fill-string-empty.js

function main(): void {
  assertSameValue("abc".padEnd(5, ""), "abc");
}
