// test262: test/built-ins/Array/prototype/splice/S15.4.4.12_A2.1_T1.js
// Adapted: getClass probe drops; fractional start truncates toward zero.

function main(): void {
  const x = [0, 1, 2, 3];
  const arr = x.splice(1.5, 3);

  assert(arr.length === 3, "x = [0,1,2,3]; arr = x.splice(1.5,3); arr.length === 3");
  assert(arr[0] === 1, "x = [0,1,2,3]; arr = x.splice(1.5,3); arr[0] === 1");
  assert(arr[1] === 2, "x = [0,1,2,3]; arr = x.splice(1.5,3); arr[1] === 2");
  assert(arr[2] === 3, "x = [0,1,2,3]; arr = x.splice(1.5,3); arr[2] === 3");

  assert(x.length === 1, "x = [0,1,2,3]; x.splice(1.5,3); x.length === 1");
  assert(x[0] === 0, "x = [0,1,2,3]; x.splice(1.5,3); x[0] === 0");
}
