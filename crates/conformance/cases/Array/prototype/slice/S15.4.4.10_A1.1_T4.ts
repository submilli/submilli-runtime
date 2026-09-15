// test262: test/built-ins/Array/prototype/slice/S15.4.4.10_A1.1_T4.js
// Adapted: the getClass/undefined probes drop (no prototypes, OOB reads
// throw); the start-at-length behavior is unchanged.

function main(): void {
  const x = [0, 1, 2, 3, 4];
  const arr = x.slice(5, 5);

  assert(arr.length === 0, "x = [0,1,2,3,4]; x.slice(5,5).length === 0");
  assert(arr.at(0) === null, "x = [0,1,2,3,4]; x.slice(5,5).at(0) === null");
}
