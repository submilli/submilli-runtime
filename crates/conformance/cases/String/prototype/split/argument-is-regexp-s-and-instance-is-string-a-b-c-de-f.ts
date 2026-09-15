// test262: test/built-ins/String/prototype/split/argument-is-regexp-s-and-instance-is-string-a-b-c-de-f.js
// `new String(...)` receiver replaced by the plain string; the constructor check is dropped.

function main(): void {
  const parts = "a b c de f".split(/\s/);
  assertCompareArray(parts, ["a", "b", "c", "de", "f"], "split on \\s");
}
