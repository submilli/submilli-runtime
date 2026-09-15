// test262: test/built-ins/Array/prototype/join/S15.4.4.5_A1.3_T1.js
// expect-fail: ECMA says undefined/null elements join as the empty string; a null element makes join trap instead of producing ""
// Adapted: undefined -> null throughout (one null-or-number element type).

type NumberOrNull = number | null;

function main(): void {
  let x: NumberOrNull[] = [null];
  assert(x.join() === "", 'x = [null]; x.join() === ""');

  x = [null, 1, null, 3];
  assert(x.join() === ",1,,3", 'x = [null,1,null,3]; x.join() === ",1,,3"');
}
