// test262: test/built-ins/Array/prototype/copyWithin/negative-start.js

function main(): void {
  assertCompareArray(
    [0, 1, 2, 3].copyWithin(0, -1), [3, 1, 2, 3],
    "[0, 1, 2, 3].copyWithin(0, -1) must return [3, 1, 2, 3]",
  );

  assertCompareArray(
    [0, 1, 2, 3, 4].copyWithin(2, -2), [0, 1, 3, 4, 4],
    "[0, 1, 2, 3, 4].copyWithin(2, -2) must return [0, 1, 3, 4, 4]",
  );

  assertCompareArray(
    [0, 1, 2, 3, 4].copyWithin(1, -2), [0, 3, 4, 3, 4],
    "[0, 1, 2, 3, 4].copyWithin(1, -2) must return [0, 3, 4, 3, 4]",
  );

  assertCompareArray(
    [0, 1, 2, 3].copyWithin(-1, -2), [0, 1, 2, 2],
    "[0, 1, 2, 3].copyWithin(-1, -2) must return [0, 1, 2, 2]",
  );

  assertCompareArray(
    [0, 1, 2, 3, 4].copyWithin(-2, -3), [0, 1, 2, 2, 3],
    "[0, 1, 2, 3, 4].copyWithin(-2, -3) must return [0, 1, 2, 2, 3]",
  );

  assertCompareArray(
    [0, 1, 2, 3, 4].copyWithin(-5, -2), [3, 4, 2, 3, 4],
    "[0, 1, 2, 3, 4].copyWithin(-5, -2) must return [3, 4, 2, 3, 4]",
  );
}
