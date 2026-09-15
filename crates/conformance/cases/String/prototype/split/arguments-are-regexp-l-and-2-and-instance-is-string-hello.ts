// test262: test/built-ins/String/prototype/split/arguments-are-regexp-l-and-2-and-instance-is-string-hello.js
// `new String(...)` receiver replaced by the plain string; the constructor check is dropped.

function main(): void {
  const parts = "hello".split(/l/, 2);
  assertCompareArray(parts, ["he", ""], "limit truncates the regex split");
}
