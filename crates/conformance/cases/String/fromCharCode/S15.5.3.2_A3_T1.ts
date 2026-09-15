// test262: test/built-ins/String/fromCharCode/S15.5.3.2_A3_T1.js

function main(): void {
  assertSameValue(String.fromCharCode(65, 66, 66, 65), "ABBA", 'String.fromCharCode(65,66,66,65) === "ABBA"');
}
