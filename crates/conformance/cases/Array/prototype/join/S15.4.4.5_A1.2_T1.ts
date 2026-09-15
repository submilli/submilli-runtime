// test262: test/built-ins/Array/prototype/join/S15.4.4.5_A1.2_T1.js
// Adapted: the sparse-array blocks are dropped (holes rejected by design);
// the default-separator rows port directly.

function main(): void {
  const x = [0, 1, 2, 3];
  assert(x.join() === "0,1,2,3", 'x = [0,1,2,3]; x.join() === "0,1,2,3"');

  const single = [0];
  assert(single.join() === "0", 'x = [0]; x.join() === "0"');
}
