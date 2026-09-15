// test262: test/built-ins/Array/prototype/unshift/S15.4.4.13_A1_T1.js
// Adapted: the just-unshifted slot is the only defined one, so the original's
// x[1] === undefined probes become at() === null checks.

function main(): void {
  const x: number[] = [];
  let unshift = x.unshift(1);
  assert(unshift === 1, "x = []; x.unshift(1) === 1");
  assert(x[0] === 1, "x = []; x.unshift(1); x[0] === 1");

  unshift = x.unshift();
  assert(unshift === 1, "x.unshift(1); x.unshift() === 1");
  assert(x.at(1) === null, "x.unshift(1); x.unshift(); x.at(1) === null");

  unshift = x.unshift(-1);
  assert(unshift === 2, "x.unshift(1); x.unshift(); x.unshift(-1) === 2");
  assert(x[0] === -1, "x.unshift(1); x.unshift(-1); x[0] === -1");
  assert(x[1] === 1, "x.unshift(1); x.unshift(-1); x[1] === 1");
  assert(x.length === 2, "x.unshift(1); x.unshift(); x.unshift(-1); x.length === 2");
}
