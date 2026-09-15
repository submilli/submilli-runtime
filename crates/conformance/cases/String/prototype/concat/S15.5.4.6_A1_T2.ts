// test262: test/built-ins/String/prototype/concat/S15.5.4.6_A1_T2.js
// The Boolean receiver and coerced arguments become explicit strings, and
// the variadic call becomes a chain — concat takes one string argument here.

function main(): void {
  assertSameValue(
    "false".concat("A").concat("true").concat("2"),
    "falseAtrue2",
    'concat appends each piece in order',
  );
}
