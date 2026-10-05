// Interpolations convert to strings the same way wherever `${` sits: after
// non-ASCII text, back to back, across lines, padded, and nested.
function main(): void {
  const n = 7;
  const ok = true;
  const word = "é";
  const out = `→${n}${ok}${ word }
${`[${n * 2}]`}`;
  assert(out === "→7trueé\n[14]", "conversions");
}
