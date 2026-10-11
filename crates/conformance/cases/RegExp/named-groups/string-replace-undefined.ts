// test262: test/built-ins/RegExp/named-groups/string-replace-undefined.js

function main(): void {
  const source = "(?<fst>.)(?<snd>.)|(?<thd>x)";
  for (const flags of ["g", "gu"]) {
    const re = new RegExp(source, flags);
    assertSameValue("abcd".replace(re, "$<thd>"), "", "global: an unreached group is empty");
  }
  for (const flags of ["", "u"]) {
    const re = new RegExp(source, flags);
    assertSameValue("abcd".replace(re, "$<thd>"), "cd", "an unreached group is empty");
  }
}
