// test262: test/built-ins/String/fromCodePoint/return-string-value.js

function main(): void {
  assertSameValue(String.fromCodePoint(0), "\0");
  assertSameValue(String.fromCodePoint(42), "*");
  assertSameValue(String.fromCodePoint(65, 90), "AZ");
  assertSameValue(String.fromCodePoint(0x404), "Є");
  assertSameValue(String.fromCodePoint(0x2f804), "你");
  assertSameValue(String.fromCodePoint(194564), "你");
  assertSameValue(
    String.fromCodePoint(0x1d306, 0x61, 0x1d307),
    "𝌆a𝌇",
  );
  assertSameValue(String.fromCodePoint(1114111), "􏿿");
}
