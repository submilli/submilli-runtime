// test262: test/built-ins/Array/prototype/slice/S15.4.4.10_A1.1_T4.js
// Adapted: the getClass probe drops (no prototypes); out-of-range arr[i]
// throws, so the undefined probe reads through arr.at(i).

function main(): void {
  const x = [0, 1, 2, 3, 4];
  const arr = x.slice(5, 5);

  assert(arr.length === 0, "x = [0,1,2,3,4]; x.slice(5,5).length === 0");
  assert(arr.at(0) === undefined, "x = [0,1,2,3,4]; x.slice(5,5).at(0) === undefined");
}
