// test262: test/built-ins/Array/prototype/join/S15.4.4.5_A1.3_T1.js
// Adapted: an explicit union element type replaces the dynamic array.

type Element = number | null | undefined;

function main(): void {
  let x: Element[] = [undefined];
  assert(x.join() === "", 'x = [undefined]; x.join() === ""');

  x = [null];
  assert(x.join() === "", 'x = [null]; x.join() === ""');

  x = [undefined, 1, null, 3];
  assert(x.join() === ",1,,3", 'x = [undefined,1,null,3]; x.join() === ",1,,3"');
}
