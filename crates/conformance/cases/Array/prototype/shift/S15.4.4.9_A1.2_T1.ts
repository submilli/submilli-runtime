// test262: test/built-ins/Array/prototype/shift/S15.4.4.9_A1.2_T1.js
// Adapted: the sparse-array blocks are dropped; the dense rows port directly.

function main(): void {
  const x = [0, 1, 2, 3];
  const shift = x.shift();
  assert(shift === 0, "x = [0,1,2,3]; x.shift() === 0");
  assert(x.length === 3, "x = [0,1,2,3]; x.shift(); x.length == 3");
  assert(x[0] === 1, "x = [0,1,2,3]; x.shift(); x[0] == 1");
  assert(x[1] === 2, "x = [0,1,2,3]; x.shift(); x[1] == 2");
}
