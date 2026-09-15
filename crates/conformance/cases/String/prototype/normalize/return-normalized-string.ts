// test262: test/built-ins/String/prototype/normalize/return-normalized-string.js
// The original's `\xC5` escapes are spelled `\u00C5` (the lexer has no \xHH form).

function main(): void {
  const s = "\u1E9B\u0323";

  assertSameValue(s.normalize("NFC"), "\u1E9B\u0323", "Normalized on NFC");
  assertSameValue(s.normalize("NFD"), "\u017F\u0323\u0307", "Normalized on NFD");
  assertSameValue(s.normalize("NFKC"), "\u1E69", "Normalized on NFKC");
  assertSameValue(s.normalize("NFKD"), "\u0073\u0323\u0307", "Normalized on NFKD");

  assertSameValue(
    "\u00C5\u2ADC\u0958\u2126\u0344".normalize("NFC"),
    "\u00C5\u2ADD\u0338\u0915\u093C\u03A9\u0308\u0301",
    "Normalized on NFC",
  );

  assertSameValue(
    "\u00C5\u2ADC\u0958\u2126\u0344".normalize("NFD"),
    "A\u030A\u2ADD\u0338\u0915\u093C\u03A9\u0308\u0301",
    "Normalized on NFD",
  );

  assertSameValue(
    "\u00C5\u2ADC\u0958\u2126\u0344".normalize("NFKC"),
    "\u00C5\u2ADD\u0338\u0915\u093C\u03A9\u0308\u0301",
    "Normalized on NFKC",
  );

  assertSameValue(
    "\u00C5\u2ADC\u0958\u2126\u0344".normalize("NFKD"),
    "A\u030A\u2ADD\u0338\u0915\u093C\u03A9\u0308\u0301",
    "Normalized on NFKD",
  );
}
