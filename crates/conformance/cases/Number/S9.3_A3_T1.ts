// test262: test/built-ins/Number/S9.3_A3_T1.js
// expect-error: expected `string | bigint | undefined`
//
// ToNumber(boolean) does not exist: `Number(x)` takes `string | bigint |
// undefined` (spec.md "Numeric globals"), so the standard's
// `Number(false) === +0` is a compile error.

function main(): void {
  assertSameValue(Number(false), +0, 'Number(false) must return +0');
  assertSameValue(Number(true), 1, 'Number(true) must return 1');
}
