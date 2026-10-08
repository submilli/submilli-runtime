// `\s` is JavaScript's WhiteSpace plus LineTerminator set, not Unicode
// `White_Space`: it takes U+FEFF and leaves out U+0085. `\d`, `\w` and `\b`
// stay ASCII even under the `u` flag, and a negated shorthand inside a class
// joins the class rather than adding a literal `^`.
const SPACES = [
  0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0xa0, 0x1680, 0x2000, 0x2005, 0x200a, 0x2028, 0x2029,
  0x202f, 0x205f, 0x3000, 0xfeff,
];
const NOT_SPACES = [0x85, 0x180e, 0x200b, 0x41];

function main(): void {
  for (const unit of SPACES) {
    const c = String.fromCharCode(unit);
    const name = unit.toString(16);
    assert(/^\s$/.test(c) && /^\s$/u.test(c), name + " is \\s");
    assert(/^[\s]$/.test(c) && /^[a\s]$/.test(c), name + " is in [\\s]");
    assert(!/\S/.test(c) && !/[^\s]/.test(c), name + " is not \\S");
  }
  for (const unit of NOT_SPACES) {
    const c = String.fromCharCode(unit);
    const name = unit.toString(16);
    assert(!/\s/.test(c) && !/\s/u.test(c), name + " is not \\s");
    assert(/^\S$/.test(c) && /^[a\S]$/.test(c), name + " is \\S");
  }
  assert(!/[a\S]/.test(" "), "[a\\S] doesn't match a space");
  assert("a　b c".split(/\s/).join("|") === "a|b|c", "split on an ideographic space");

  const arabicThree = String.fromCharCode(0x663);
  const eAcute = String.fromCharCode(0xe9);
  assert(!/\d/u.test(arabicThree) && !/\d/.test(arabicThree), "\\d is ASCII");
  assert(!/\w/u.test(eAcute) && !/\w/.test(eAcute), "\\w is ASCII");
  assert(/\bx/.test(eAcute + "x") && /\bx/u.test(eAcute + "x"), "\\b sees an accented letter as a non-word");
  assert(!/\Bx/.test(eAcute + "x"), "\\B is the complement");
  assert(/^[\b]$/.test(String.fromCharCode(8)), "[\\b] is a backspace");
}
