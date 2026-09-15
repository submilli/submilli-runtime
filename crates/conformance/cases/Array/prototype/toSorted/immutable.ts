// test262: test/built-ins/Array/prototype/toSorted/immutable.js
// Adapted: notSameValue over arrays compares structurally here, which still
// distinguishes the sorted copy from the unsorted receiver.

function main(): void {
  const arr = [2, 0, 1];
  arr.toSorted();

  assertCompareArray(arr, [2, 0, 1]);
  assertNotSameValue(arr.toSorted(), arr);
}
