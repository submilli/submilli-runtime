// test262: test/built-ins/RegExp/S15.10.2.10_A2.1_T1.js
// expect-fail: control escapes \cA..\cZ are valid ECMA-262 syntax; the engine rejects them ("unrecognized escape sequence"), and the dynamic-constructor failure surfaces as a trap

function main(): void {
  let result = true;
  for (let alpha = 65; alpha <= 90; alpha++) {
    const str = String.fromCharCode(alpha % 32);
    const arr = new RegExp("\\c" + String.fromCharCode(alpha), "").exec(str);
    if (arr === null) {
      result = false;
    } else {
      const matched = arr.match;
      if (matched !== str) {
        result = false;
      }
    }
  }
  assertSameValue(result, true, "\\cX matches the control character X % 32");
}
