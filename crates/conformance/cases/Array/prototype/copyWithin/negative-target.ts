// test262: test/built-ins/Array/prototype/copyWithin/negative-target.js

function main(): void {
  assertCompareArray(
    [0, 1, 2, 3].copyWithin(-1, 0), [0, 1, 2, 0],
    "[0, 1, 2, 3].copyWithin(-1, 0) must return [0, 1, 2, 0]",
  );

  assertCompareArray(
    [0, 1, 2, 3, 4].copyWithin(-2, 2), [0, 1, 2, 2, 3],
    "[0, 1, 2, 3, 4].copyWithin(-2, 2) must return [0, 1, 2, 2, 3]",
  );

  assertCompareArray(
    [0, 1, 2, 3].copyWithin(-1, 2), [0, 1, 2, 2],
    "[0, 1, 2, 3].copyWithin(-1, 2) must return [0, 1, 2, 2]",
  );
}
