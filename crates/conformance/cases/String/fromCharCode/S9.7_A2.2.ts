// test262: test/built-ins/String/fromCharCode/S9.7_A2.2.js

function main(): void {
  assertSameValue(String.fromCharCode(-32767).charCodeAt(0), 32769, "#1: fromCharCode(-32767)");
  assertSameValue(String.fromCharCode(-32768).charCodeAt(0), 32768, "#2: fromCharCode(-32768)");
  assertSameValue(String.fromCharCode(-32769).charCodeAt(0), 32767, "#3: fromCharCode(-32769)");
  assertSameValue(String.fromCharCode(-65535).charCodeAt(0), 1, "#4: fromCharCode(-65535)");
  assertSameValue(String.fromCharCode(-65536).charCodeAt(0), 0, "#5: fromCharCode(-65536)");
  assertSameValue(String.fromCharCode(-65537).charCodeAt(0), 65535, "#6: fromCharCode(-65537)");
  assertSameValue(String.fromCharCode(65535).charCodeAt(0), 65535, "#7: fromCharCode(65535)");
  assertSameValue(String.fromCharCode(65536).charCodeAt(0), 0, "#8: fromCharCode(65536)");
  assertSameValue(String.fromCharCode(65537).charCodeAt(0), 1, "#9: fromCharCode(65537)");
  assertSameValue(String.fromCharCode(131071).charCodeAt(0), 65535, "#10: fromCharCode(131071)");
  assertSameValue(String.fromCharCode(131072).charCodeAt(0), 0, "#11: fromCharCode(131072)");
  assertSameValue(String.fromCharCode(131073).charCodeAt(0), 1, "#12: fromCharCode(131073)");
}
