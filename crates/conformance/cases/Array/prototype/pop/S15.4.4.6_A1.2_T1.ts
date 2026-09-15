// test262: test/built-ins/Array/prototype/pop/S15.4.4.6_A1.2_T1.js
// Adapted: the sparse-array and length-truncation blocks are dropped; the
// removed slot is checked via at() (indexed OOB reads throw here).

function main(): void {
  const x = [0, 1, 2, 3];
  const pop = x.pop();
  assert(pop === 3, "x = [0,1,2,3]; x.pop() === 3");
  assert(x.length === 3, "x = [0,1,2,3]; x.pop(); x.length == 3");
  assert(x.at(3) === null, "x = [0,1,2,3]; x.pop(); x.at(3) == null");
  assert(x[2] === 2, "x = [0,1,2,3]; x.pop(); x[2] == 2");
}
