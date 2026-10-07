// test262: test/built-ins/NaN/S15.1.1.1_A1.js
// expect-error: this comparison is always `true`: `NaN` is not equal to any value
//
// Comparing with the global `NaN` is a compile error, as in TypeScript (TS2845):
// the answer never depends on the other operand.

function main(): void {
  assert(NaN !== NaN, "NaN !== NaN");
}
