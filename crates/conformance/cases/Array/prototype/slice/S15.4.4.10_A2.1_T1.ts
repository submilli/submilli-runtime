// test262: test/built-ins/Array/prototype/slice/S15.4.4.10_A2.1_T1.js
// Adapted: the getClass probe drops (no prototypes); out-of-range arr[i]
// throws, so the undefined probe reads through arr.at(i).

function main(): void {
  const x = [0, 1, 2, 3, 4];
  const arr = x.slice(2.5, 4);

  assert(arr.length === 2, "x = [0,1,2,3,4]; x.slice(2.5,4).length === 2");
  assert(arr[0] === 2, "x = [0,1,2,3,4]; x.slice(2.5,4)[0] === 2");
  assert(arr[1] === 3, "x = [0,1,2,3,4]; x.slice(2.5,4)[1] === 3");
  assert(arr.at(3) === undefined, "x = [0,1,2,3,4]; x.slice(2.5,4).at(3) === undefined");
}
