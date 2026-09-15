// test262: test/built-ins/Array/prototype/toSpliced/start-neg-subtracted-from-length.js

function main(): void {
  const result = [0, 1, 2, 3, 4].toSpliced(-3, 2);
  assertCompareArray(result, [0, 1, 4]);
}
