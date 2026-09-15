// test262: test/built-ins/String/prototype/padStart/fill-string-empty.js

function main(): void {
  assertSameValue("abc".padStart(5, ""), "abc");
}
