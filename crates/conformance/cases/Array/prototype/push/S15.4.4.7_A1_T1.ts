// test262: test/built-ins/Array/prototype/push/S15.4.4.7_A1_T1.js
// Adapted: this port retains the single-element calls and omits the upstream
// zero-argument blocks. S15.4.4.7_A1_T2 covers variadic push.

function main(): void {
  const x: number[] = [];
  let push = x.push(1);
  assert(push === 1, "x = []; x.push(1) === 1");
  assert(x[0] === 1, "x = []; x.push(1); x[0] === 1");

  push = x.push(-1);
  assert(push === 2, "x.push(1); x.push(-1) === 2");
  assert(x[1] === -1, "x.push(1); x.push(-1); x[1] === -1");
  assert(x.length === 2, "x.push(1); x.push(-1); x.length === 2");
}
