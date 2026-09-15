// test262: test/built-ins/Array/prototype/concat/S15.4.4.4_A1_T2.js
// expect-fail: standard concat appends non-array arguments as elements; our concat only accepts arrays (concat(...others: T[][])), so a scalar argument is a compile error
// Adapted: numeric elements only; the point — concat of a scalar argument —
// is preserved.

function main(): void {
  const x = [0];
  const z = [1, 2];
  const arr = x.concat(z, -1);

  assert(arr[0] === 0, "The value of arr[0] is expected to be 0");
  assert(arr[1] === 1, "The value of arr[1] is expected to be 1");
  assert(arr[2] === 2, "The value of arr[2] is expected to be 2");
  assert(arr[3] === -1, "The value of arr[3] is expected to be -1");
  assert(arr.length === 4, "The value of arr.length is expected to be 4");
}
