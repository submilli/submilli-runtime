// test262: test/built-ins/Array/prototype/join/S15.4.4.5_A1.1_T1.js
// Adapted: `new Array()` / the length-truncation trick become a plain empty
// literal; the point (empty array joins to "") is unchanged.

function main(): void {
  const x: number[] = [];
  assert(x.join() === "", 'x = []; x.join() === ""');
}
