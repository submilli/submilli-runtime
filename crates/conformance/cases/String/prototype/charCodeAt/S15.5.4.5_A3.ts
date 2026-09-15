// test262: test/built-ins/String/prototype/charCodeAt/S15.5.4.5_A3.js
// `new String("ABC")` wrapper object replaced by the plain string.

function main(): void {
  assertSameValue("ABC".charCodeAt(3), NaN);
}
