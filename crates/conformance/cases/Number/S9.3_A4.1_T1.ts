// test262: test/built-ins/Number/S9.3_A4.1_T1.js
// expect-error: expected `string | bigint | undefined`
//
// ToNumber(boolean) does not exist: Number(x) takes `string | bigint` only
// (spec.md "Numeric globals" — other types are rejected by the union
// parameter), so the standard's Number(true) === 1 is a compile error.

function main(): void {
  assertSameValue(Number(true), 1, "Number(true) must return 1");
  assertSameValue(Number(false), 0, "Number(false) must return 0");
}
