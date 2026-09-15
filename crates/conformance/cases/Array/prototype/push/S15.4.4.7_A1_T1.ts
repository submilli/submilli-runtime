// test262: test/built-ins/Array/prototype/push/S15.4.4.7_A1_T1.js
// Adapted: the zero-argument push() blocks are dropped — push takes exactly
// one element here (see the expect-fail variadic case S15.4.4.7_A1_T2).

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
