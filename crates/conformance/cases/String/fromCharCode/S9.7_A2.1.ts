// test262: test/built-ins/String/fromCharCode/S9.7_A2.1.js

function main(): void {
  assertSameValue(String.fromCharCode(0).charCodeAt(0), 0, "#1: fromCharCode(0)");
  assertSameValue(String.fromCharCode(1).charCodeAt(0), 1, "#2: fromCharCode(1)");
  assertSameValue(String.fromCharCode(-1).charCodeAt(0), 65535, "#3: fromCharCode(-1)");
  assertSameValue(String.fromCharCode(65535).charCodeAt(0), 65535, "#4: fromCharCode(65535)");
  assertSameValue(String.fromCharCode(65534).charCodeAt(0), 65534, "#5: fromCharCode(65534)");
  assertSameValue(String.fromCharCode(65536).charCodeAt(0), 0, "#6: fromCharCode(65536)");
  assertSameValue(String.fromCharCode(4294967295).charCodeAt(0), 65535, "#7: fromCharCode(4294967295)");
  assertSameValue(String.fromCharCode(4294967294).charCodeAt(0), 65534, "#8: fromCharCode(4294967294)");
  assertSameValue(String.fromCharCode(4294967296).charCodeAt(0), 0, "#9: fromCharCode(4294967296)");
}
