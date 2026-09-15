// test262: test/built-ins/String/prototype/charAt/S15.5.4.4_A3.js
// `new String("ABC")` wrapper object replaced by the plain string.

function main(): void {
  assertSameValue("ABC".charAt(3), "", 'charAt(pos >= length) === ""');
}
