// test262: test/built-ins/Array/prototype/toSpliced/deleteCount-clamped-between-zero-and-remaining-count.js

function main(): void {
  assertCompareArray(
    [0, 1, 2, 3, 4, 5].toSpliced(2, -1),
    [0, 1, 2, 3, 4, 5],
  );

  assertCompareArray(
    [0, 1, 2, 3, 4, 5].toSpliced(-4, -1),
    [0, 1, 2, 3, 4, 5],
  );

  assertCompareArray(
    [0, 1, 2, 3, 4, 5].toSpliced(2, 6),
    [0, 1],
  );

  assertCompareArray(
    [0, 1, 2, 3, 4, 5].toSpliced(-4, 6),
    [0, 1],
  );
}
