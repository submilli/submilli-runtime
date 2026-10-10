function main(): void {
  const lone = String.fromCharCode(0xD800);
  assert(("a" + lone + "ß").toUpperCase() === "A" + lone + "SS", "uppercase preserves UTF-16");
  assert(("İ" + lone + "ΟΣ").toLowerCase() === "i\u0307" + lone + "ος", "lowercase preserves UTF-16 and final sigma");
  assert("ΟΣ\u0301".toLowerCase() === "ος\u0301", "final sigma skips marks");
  assert("ΟΣ\u0301Α".toLowerCase() === "οσ\u0301α", "non-final sigma skips marks");
  assert("Σ".toLowerCase() === "σ", "sigma requires preceding cased character");
  assert(("AΣ" + lone + "A").toLowerCase() === "aς" + lone + "a", "surrogates terminate sigma context");
  assert("\uFEFF x \uFEFF".trim() === "x", "BOM is ECMAScript whitespace");
  assert("\u0085x\u0085".trim() === "\u0085x\u0085", "NEL is not ECMAScript whitespace");
  assert((" "+lone+" ").trim() === lone, "trim preserves lone surrogate");
  assert("   ".trim() === "", "all whitespace");
  assert(" x ".trimStart() === "x ", "trimStart keeps suffix");
  assert(" x ".trimEnd() === " x", "trimEnd keeps prefix");
  for (let count = 128; count <= 256; count *= 2) {
    const text = ("AΣ\u0301 "+lone).repeat(count);
    assert(text.toLowerCase().length === text.length, "linear sigma context input");
    assert((" "+text+" ").trim().length === text.length, "linear trimming input");
  }
}
