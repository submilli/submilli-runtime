// test262: test/built-ins/Array/prototype/toSorted/comparefn-controls-sort.js

function numericCompare(a: number, b: number): number {
  return a - b;
}

function reverseNumericCompare(a: number, b: number): number {
  return b - a;
}

function main(): void {
  assertCompareArray([1, 2, 3, 4].toSorted(numericCompare), [1, 2, 3, 4]);
  assertCompareArray([4, 3, 2, 1].toSorted(numericCompare), [1, 2, 3, 4]);
  assertCompareArray(
    [333, 33, 3, 222, 22, 2, 111, 11, 1].toSorted(numericCompare),
    [1, 2, 3, 11, 22, 33, 111, 222, 333],
  );

  assertCompareArray([1, 2, 3, 4].toSorted(reverseNumericCompare), [4, 3, 2, 1]);
  assertCompareArray([4, 3, 2, 1].toSorted(reverseNumericCompare), [4, 3, 2, 1]);
  assertCompareArray(
    [333, 33, 3, 222, 22, 2, 111, 11, 1].toSorted(reverseNumericCompare),
    [333, 222, 111, 33, 22, 11, 3, 2, 1],
  );
}
