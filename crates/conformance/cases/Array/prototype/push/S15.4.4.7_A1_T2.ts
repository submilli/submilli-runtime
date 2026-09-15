// test262: test/built-ins/Array/prototype/push/S15.4.4.7_A1_T2.js
// expect-fail: standard push is variadic (push(...items)); our push takes exactly one element, so a multi-argument push is a compile error
// Adapted: numeric elements only (the original mixes booleans and strings).

function main(): void {
  const x: number[] = [];
  assert(x.length === 0, "x = []; x.length === 0");

  x.push(0);
  const push = x.push(1, 2, 3);
  assert(push === 4, "x.push(1, 2, 3) === 4");
  assert(x[0] === 0, "x[0] === 0");
  assert(x[1] === 1, "x[1] === 1");
  assert(x[2] === 2, "x[2] === 2");
  assert(x[3] === 3, "x[3] === 3");
  assert(x.length === 4, "x.length === 4");
}
