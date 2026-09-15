// test262: test/built-ins/String/prototype/normalize/return-normalized-string-using-default-parameter.js
// The `normalize(undefined)` row is dropped — no `undefined` in the language.

function main(): void {
  const s = "\u00C5\u2ADC\u0958\u2126\u0344";
  const nfc = "\u00C5\u2ADD\u0338\u0915\u093C\u03A9\u0308\u0301";

  assertSameValue(s.normalize(), nfc, "Use NFC as the default form");
}
