// test262: test/built-ins/RegExp/named-groups/string-replace-nocaptures.js

function main(): void {
  const source = "(.)(.)|(x)";
  for (const flags of ["", "u"]) {
    const re = new RegExp(source, flags);
    assertSameValue("abcd".replace(re, "$<snd>$<fst>"), "$<snd>$<fst>cd", "$< is literal without named groups");
    assertSameValue("abcd".replace(re, "$2$1"), "bacd", "numbered groups");
    assertSameValue("abcd".replace(re, "$3"), "cd", "an unreached numbered group is empty");
    assertSameValue("abcd".replace(re, "$<snd"), "$<sndcd", "an unclosed $< is literal");
    assertSameValue("abcd".replace(re, "$<snd$1"), "$<sndacd", "a group after an unclosed $<");
    assertSameValue("abcd".replace(re, "$<42$1>"), "$<42a>cd", "a group inside a literal $<");
    assertSameValue("abcd".replace(re, "$<fth>"), "$<fth>cd", "an unknown name stays literal");
    assertSameValue("abcd".replace(re, "$<$1>"), "$<a>cd", "a group inside $<>");
  }
}
