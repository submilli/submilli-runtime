// test262: test/built-ins/String/prototype/charCodeAt/S15.5.4.5_A2.js
// The prototype-borrowing receiver is replaced by a plain string;
// the intent (negative pos returns NaN) is unchanged.

function main(): void {
  assertSameValue("ABC".charCodeAt(-1), NaN);
}
