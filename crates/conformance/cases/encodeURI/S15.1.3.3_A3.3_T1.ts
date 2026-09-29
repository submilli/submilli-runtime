// test262: test/built-ins/encodeURI/S15.1.3.3_A3.3_T1.js

function main(): void {
  assertSameValue(encodeURI("#"), "#", "#1: unescapedURISet containing \"#\"");
}
