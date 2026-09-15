// test262: test/built-ins/String/prototype/repeat/count-is-zero-returns-empty-string.js

function main(): void {
  assertSameValue("foo".repeat(0), "");
}
