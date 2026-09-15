// test262: test/built-ins/Array/prototype/toSorted/comparefn-default.js
// Adapted: the ['a', 2, 1, 'z'] mixed-type row is dropped (homogeneous
// arrays); the numeric rows pin the default string-order comparison.

function main(): void {
  assertCompareArray([1, 2, 3, 4].toSorted(), [1, 2, 3, 4]);
  assertCompareArray([4, 3, 2, 1].toSorted(), [1, 2, 3, 4]);

  assertCompareArray(
    [333, 33, 3, 222, 22, 2, 111, 11, 1].toSorted(),
    [1, 11, 111, 2, 22, 222, 3, 33, 333],
  );
}
