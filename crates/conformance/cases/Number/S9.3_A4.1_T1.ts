// test262: test/built-ins/Number/S9.3_A4.1_T1.js
// expect-error: expected `string | bigint | undefined`
//
// Number(x) takes `string | bigint | undefined`; a number argument is rejected
// by the union parameter (no ToNumber(number) call), so the standard's
// Number(13) === 13 is a compile error.

function main(): void {
  assertSameValue(Number(13), 13, "Number(13) must return 13");
  assertSameValue(Number(-13), -13, "Number(-13) must return -13");
  assertSameValue(Number(1.3), 1.3, "Number(1.3) must return 1.3");
  assertSameValue(Number(-1.3), -1.3, "Number(-1.3) must return -1.3");
}
