// test262: test/built-ins/Array/prototype/toReversed/immutable.js
// Adapted: notSameValue over arrays compares structurally here, which still
// distinguishes the reversed copy from the receiver.

function main(): void {
  const arr = [0, 1, 2];
  arr.toReversed();

  assertCompareArray(arr, [0, 1, 2]);
  assertNotSameValue(arr.toReversed(), arr);
  assertCompareArray(arr.toReversed(), [2, 1, 0]);
}
