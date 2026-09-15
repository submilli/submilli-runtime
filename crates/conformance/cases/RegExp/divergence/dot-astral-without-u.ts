// Divergence pin (docs/regex.md): without the u flag JS `.` operates on UTF-16
// code units, so a supplementary-plane character (two code units) is never
// matched by a single `.`; this engine always matches whole code points, with
// or without the u flag.

function main(): void {
  const astral = String.fromCodePoint(66304);
  assertSameValue(astral.length, 2, "astral character spans two code units");
  assertSameValue(/^.$/.test(astral), true, "no u: single . matches the whole code point (JS: false)");
  assertSameValue(/^.$/u.test(astral), true, "u: single . matches the whole code point (JS agrees)");
}
