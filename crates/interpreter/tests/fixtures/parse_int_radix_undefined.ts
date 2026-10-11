// An omitted or `undefined` radix detects a `0x` prefix and otherwise reads
// base 10, as in JavaScript.
function main(): void {
  const none: number | undefined = undefined;
  const hex: number | undefined = 16;
  assert(parseInt("0x1f") === 31, "an omitted radix reads a 0x prefix as hex");
  assert(parseInt("08") === 8, "an omitted radix is otherwise base 10");
  assert(Number.isNaN(parseInt("ff", none)), "an undefined radix is base 10");
  assert(parseInt("ff", hex) === 255, "a present radix is used");
  assert(Number.parseInt("0x10", undefined) === 16, "Number.parseInt matches");
  // The radix goes through ToInt32: 0 detects, 2–36 parse, anything else is NaN.
  assert(Number.isNaN(parseInt("11", 37)) && Number.isNaN(parseInt("11", -1)), "out of range");
  assert(parseInt("ff", 4294967312) === 255, "ToInt32 wraps a radix past 2^32");
  assert(parseInt("11", 36.9) === 37, "a fractional radix truncates");
  assert(Number.isNaN(parseInt("0x")) && Number.isNaN(parseInt("-0x")), "a bare 0x has no digits");
  assert(Number.isNaN(parseInt("0x", 16)) && Number.isNaN(Number.parseInt("0x", 0)), "with a radix too");
  assert(parseInt(" -0x1F") === -31 && parseInt("0x0x1") === 0, "whitespace, sign and a second prefix");
  assert(parseInt("\v7") === 7 && parseFloat("\ufeff2") === 2, "JavaScript whitespace is skipped");
  assert(parseInt("\u00a07") === 7 && parseInt("\u30007") === 7 && parseFloat("\u20282") === 2, "Unicode spaces and line terminators too");
  assert(Number.isNaN(parseInt("\u180e7")) && Number.isNaN(parseFloat("\u200b2")), "but not U+180E or U+200B");
  assert(Number.isNaN(Number("0x")) && Number("0x10") === 16, "Number() keeps its own prefix rule");
}
