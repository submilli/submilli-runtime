// test262: test/built-ins/String/prototype/split/argument-is-regexp-x-and-instance-is-string-a-b-c-de-f.js
// `new String(...)` receiver replaced by the plain string; the constructor check is dropped.

function main(): void {
  const parts = "a b c de f".split(/X/);
  assertCompareArray(parts, ["a b c de f"], "no match returns the whole string as one element");
}
