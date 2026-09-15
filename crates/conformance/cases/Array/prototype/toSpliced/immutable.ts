// test262: test/built-ins/Array/prototype/toSpliced/immutable.js
// Adapted: notSameValue over arrays compares structurally here, which still
// distinguishes the spliced copies from the receiver.

function main(): void {
  const arr = [2, 0, 1];
  arr.toSpliced(0, 0, -1);

  assertCompareArray(arr, [2, 0, 1]);
  assertNotSameValue(arr.toSpliced(0, 0, -1), arr);
  assertNotSameValue(arr.toSpliced(0, 1, -1), arr);
}
