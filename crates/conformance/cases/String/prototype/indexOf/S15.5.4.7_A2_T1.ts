// test262: test/built-ins/String/prototype/indexOf/S15.5.4.7_A2_T1.js

function main(): void {
  assertSameValue("abcd".indexOf("abcdab"), -1, '"abcd".indexOf("abcdab") === -1');
}
