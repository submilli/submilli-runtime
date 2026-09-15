// test262: test/built-ins/String/prototype/split/argument-is-regexp-reg-exp-d-and-instance-is-string-dfe23iu-34-65.js
// `new String(...)` receiver replaced by the plain string; the constructor check is dropped.

function main(): void {
  const re = new RegExp("\\d+", "");
  const parts = "dfe23iu 34 =+65--".split(re);
  assertCompareArray(parts, ["dfe", "iu ", " =+", "--"], "dynamically constructed regex splits like the literal");
}
