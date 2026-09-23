// test262: test/built-ins/RegExp/unicode_full_case_folding.js

function main(): void {
  assert(/[ΐ]/iu.test("ΐ"), "\\u0390 matches \\u1FD3");
  assert(/[ΐ]/iu.test("ΐ"), "\\u1FD3 matches \\u0390");
  assert(/[ΰ]/iu.test("ΰ"), "\\u03B0 matches \\u1FE3");
  assert(/[ΰ]/iu.test("ΰ"), "\\u1FE3 matches \\u03B0");
  assert(/[ﬅ]/iu.test("ﬆ"), "\\uFB05 matches \\uFB06");
  assert(/[ﬆ]/iu.test("ﬅ"), "\\uFB06 matches \\uFB05");
}
