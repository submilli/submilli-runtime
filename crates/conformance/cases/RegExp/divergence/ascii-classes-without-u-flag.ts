// Divergence pin (docs/regex.md): without the u flag, \d / \w / \s are ASCII
// classes. JS's no-u \s still matches Unicode whitespace such as NBSP; here it
// does not. With the u flag both agree on Unicode semantics.

function main(): void {
  assertSameValue(/\s/.test("\u00A0"), false, "no u: \\s is ASCII-only, NBSP does not match");
  assertSameValue(/\s/u.test("\u00A0"), true, "u: \\s matches NBSP");
  assertSameValue(/\d/.test("\u0660"), false, "no u: \\d is ASCII-only, Arabic-Indic digit does not match");
  assertSameValue(/\d/u.test("\u0660"), true, "u: \\d matches the Arabic-Indic digit");
  assertSameValue(/\s/.test(" "), true, "no u: ASCII space matches");
  assertSameValue(/\d/.test("7"), true, "no u: ASCII digit matches");
}
