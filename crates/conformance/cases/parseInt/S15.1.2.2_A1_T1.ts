// test262: test/built-ins/parseInt/S15.1.2.2_A1_T1.js
// expect-error: expected `string`, got `boolean`
//
// ToString coercion of the argument does not exist: parseInt takes a
// `string` (spec.md "Numeric globals"), so the standard's
// parseInt(true) === NaN is a compile error.

function main(): void {
  assertSameValue(parseInt(true), NaN, "parseInt(true) must return NaN");
  assertSameValue(parseInt(false), NaN, "parseInt(false) must return NaN");
}
