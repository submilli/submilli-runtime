// test262: test/built-ins/Array/prototype/copyWithin/non-negative-target-and-start.js

function main(): void {
  assertCompareArray(
    ["a", "b", "c", "d", "e", "f"].copyWithin(0, 0),
    ["a", "b", "c", "d", "e", "f"],
  );

  assertCompareArray(
    ["a", "b", "c", "d", "e", "f"].copyWithin(0, 2),
    ["c", "d", "e", "f", "e", "f"],
  );

  assertCompareArray(
    ["a", "b", "c", "d", "e", "f"].copyWithin(3, 0),
    ["a", "b", "c", "a", "b", "c"],
  );

  assertCompareArray(
    [0, 1, 2, 3, 4, 5].copyWithin(1, 4),
    [0, 4, 5, 3, 4, 5],
  );
}
