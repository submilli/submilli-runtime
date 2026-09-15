// test262: test/built-ins/Array/prototype/slice/S15.4.4.10_A1.2_T1.js
// Adapted: getClass/undefined probes drop; negative start counts from the end.

function main(): void {
  const x = [0, 1, 2, 3, 4];
  const arr = x.slice(-3, 3);

  assert(arr.length === 1, "x = [0,1,2,3,4]; x.slice(-3,3).length === 1");
  assert(arr[0] === 2, "x = [0,1,2,3,4]; x.slice(-3,3)[0] === 2");
  assert(arr.at(1) === null, "x = [0,1,2,3,4]; x.slice(-3,3).at(1) === null");
}
