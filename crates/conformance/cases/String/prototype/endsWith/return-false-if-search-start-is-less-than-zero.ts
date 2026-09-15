// test262: test/built-ins/String/prototype/endsWith/return-false-if-search-start-is-less-than-zero.js

function main(): void {
  assertSameValue("web".endsWith("w", 0), false, '"web".endsWith("w", 0) returns false');
  assertSameValue("Bob".endsWith("  Bob"), false, '"Bob".endsWith("  Bob") returns false');
}
