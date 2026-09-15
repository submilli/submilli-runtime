// test262: test/built-ins/Array/prototype/splice/S15.4.4.12_A1.1_T4.js
// Adapted: getClass probe drops; delete-and-insert plus the returned
// removed-elements array port directly.

function main(): void {
  const x = [0, 1, 2, 3];
  const arr = x.splice(1, 3, 4, 5);

  assert(arr.length === 3, "x = [0,1,2,3]; arr = x.splice(1,3,4,5); arr.length === 3");
  assert(arr[0] === 1, "x = [0,1,2,3]; arr = x.splice(1,3,4,5); arr[0] === 1");
  assert(arr[1] === 2, "x = [0,1,2,3]; arr = x.splice(1,3,4,5); arr[1] === 2");
  assert(arr[2] === 3, "x = [0,1,2,3]; arr = x.splice(1,3,4,5); arr[2] === 3");

  assert(x.length === 3, "x = [0,1,2,3]; x.splice(1,3,4,5); x.length === 3");
  assert(x[0] === 0, "x = [0,1,2,3]; x.splice(1,3,4,5); x[0] === 0");
  assert(x[1] === 4, "x = [0,1,2,3]; x.splice(1,3,4,5); x[1] === 4");
  assert(x[2] === 5, "x = [0,1,2,3]; x.splice(1,3,4,5); x[2] === 5");
}
