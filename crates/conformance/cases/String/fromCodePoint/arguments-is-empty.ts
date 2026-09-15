// test262: test/built-ins/String/fromCodePoint/arguments-is-empty.js

function main(): void {
  assertSameValue(String.fromCodePoint(), "");
}
