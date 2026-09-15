// test262: test/built-ins/String/prototype/split/argument-is-regexp-d-and-instance-is-string-dfe23iu-34-65.js
// `new String(...)` receiver replaced by the plain string; the constructor check is dropped.

function main(): void {
  const parts = "dfe23iu 34 =+65--".split(/\d+/);
  assertCompareArray(parts, ["dfe", "iu ", " =+", "--"], "regex split on digit runs");
}
