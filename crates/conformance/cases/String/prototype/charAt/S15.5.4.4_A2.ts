// test262: test/built-ins/String/prototype/charAt/S15.5.4.4_A2.js
// The prototype-borrowing receiver is replaced by a plain string;
// the intent (negative pos returns the empty string) is unchanged.

function main(): void {
  assertSameValue("ABC".charAt(-1), "", 'charAt(-1) === ""');
}
