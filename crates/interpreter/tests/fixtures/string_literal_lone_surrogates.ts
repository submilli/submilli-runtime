// A `\u` escape for a lone surrogate gives the literal that one UTF-16 code
// unit, as in JavaScript: `"\ud800"` has length 1, and two halves written
// separately join into a surrogate pair.
type High = "\ud800";
type Face = "\u{1F600}" | "x";

function describe(s: string): string {
  switch (s) {
    case "\udc00":
      return "low";
    case "\ud800":
      return "high";
    default:
      return "other";
  }
}

function main(): void {
  const high = "\ud800";
  assert(high.length === 1 && high.charCodeAt(0) === 0xd800, "a lone high surrogate is one unit");
  assert("\udc00x".charCodeAt(0) === 0xdc00, "a lone low surrogate is one unit");
  assert("\u{D800}".length === 1, "the braced form too");
  assert(high === String.fromCharCode(0xd800), "it equals the unit built at run time");
  assert(!high.isWellFormed(), "it is not well formed");

  const joined = "\ud83d" + "\ude00";
  assert(joined === "\u{1F600}" && joined.length === 2, "two halves join into a pair");
  const pair: Face = `\ud83d${"\ude00"}`;
  assert(pair.codePointAt(0) === 0x1f600, "a template joins them too");
  const braced: Face = "\u{D83D}\u{DE00}";
  assert(braced === "\u{1F600}" && braced.length === 2, "halves in one literal are one character");

  assert(describe(String.fromCharCode(0xdc00)) === "low", "a switch case matches a lone surrogate");
  const typed: High = "\ud800";
  assert(describe(typed) === "high", "a literal type holds it");
  assert(JSON.stringify(high) === '"\\ud800"', "JSON escapes it");

  const nonchar = "\u{10FFFE}\u{10FFFF}";
  assert(nonchar.length === 4 && nonchar.codePointAt(0) === 0x10fffe, "noncharacters are kept as written");

  const keyed: Record<string, number> = { "\ud800": 1 };
  assert(keyed[String.fromCharCode(0xd800)] === 1, "an object key keeps it");
}
