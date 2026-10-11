// test262: test/built-ins/RegExp/named-groups/string-replace-unclosed.js

function main(): void {
  const source = "(?<fst>.)(?<snd>.)|(?<thd>x)";
  for (const flags of ["", "u"]) {
    const re = new RegExp(source, flags);
    assertSameValue("abcd".replace(re, "$<snd"), "$<sndcd", "an unclosed $< is literal");
  }
  for (const flags of ["g", "gu"]) {
    const re = new RegExp(source, flags);
    assertSameValue("abcd".replace(re, "$<snd"), "$<snd$<snd", "global: an unclosed $< is literal");
  }
}
