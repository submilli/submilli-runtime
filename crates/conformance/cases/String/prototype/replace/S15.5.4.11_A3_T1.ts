// test262: test/built-ins/String/prototype/replace/S15.5.4.11_A3_T1.js
// The original's `"$11" + 15` concatenation is spelled "$1115" directly.

function main(): void {
  const str = "uid=31";
  const re = /(uid=)(\d+)/;

  assertSameValue(
    str.replace(re, "$1115"),
    "uid=115",
    "$11 resolves to capture 1 followed by a literal 1 when there is no capture 11",
  );
}
