// test262: test/built-ins/Array/prototype/with/index-negative.js

function main(): void {
  const arr = [0, 1, 2];

  assertCompareArray(arr.with(-1, 4), [0, 1, 4]);
  assertCompareArray(arr.with(-3, 4), [4, 1, 2]);

  // -0 is not < 0
  assertCompareArray(arr.with(-0, 4), [4, 1, 2]);
}
