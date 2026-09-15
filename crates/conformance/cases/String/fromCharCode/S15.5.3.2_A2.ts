// test262: test/built-ins/String/fromCharCode/S15.5.3.2_A2.js

function main(): void {
  assertSameValue(String.fromCharCode(), "", "String.fromCharCode() returns empty string");
}
