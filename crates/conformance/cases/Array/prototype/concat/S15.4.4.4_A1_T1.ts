// test262: test/built-ins/Array/prototype/concat/S15.4.4.4_A1_T1.js
// Adapted: getClass probe drops; new Array(...) becomes literals.

function main(): void {
  const x: number[] = [];
  const y = [0, 1];
  const z = [2, 3, 4];
  const arr = x.concat(y, z);

  assert(arr[0] === 0, "The value of arr[0] is expected to be 0");
  assert(arr[1] === 1, "The value of arr[1] is expected to be 1");
  assert(arr[2] === 2, "The value of arr[2] is expected to be 2");
  assert(arr[3] === 3, "The value of arr[3] is expected to be 3");
  assert(arr[4] === 4, "The value of arr[4] is expected to be 4");
  assert(arr.length === 5, "The value of arr.length is expected to be 5");
}
