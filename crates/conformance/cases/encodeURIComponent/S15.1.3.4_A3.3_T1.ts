// test262: test/built-ins/encodeURIComponent/S15.1.3.4_A3.3_T1.js

function main(): void {
  assertSameValue(encodeURIComponent("#"), "%23", "#1: unescapedURIComponentSet not containing \"%23\"");
}
