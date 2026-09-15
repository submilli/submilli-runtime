// test262: test/built-ins/String/prototype/search/S15.5.4.12_A3_T1.js
// `new String(...)` wrapper object replaced by the plain string.

function main(): void {
  const aString = "power of the power of the power of the power of the power of the power of the great sword";

  assertSameValue(
    aString.search(/the/),
    aString.search(/the/g),
    "search ignores the global flag",
  );
}
