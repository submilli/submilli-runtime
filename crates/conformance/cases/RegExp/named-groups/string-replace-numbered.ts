// test262: test/built-ins/RegExp/named-groups/string-replace-numbered.js

function main(): void {
  const source = "(?<fst>.)(?<snd>.)|(?<thd>x)";

  const g = new RegExp(source, "g");
  assertSameValue("abcd".replace(g, "$2$1"), "badc", "global replace swaps via numbered tokens");
  const gu = new RegExp(source, "gu");
  assertSameValue("abcd".replace(gu, "$2$1"), "badc", "global unicode replace swaps via numbered tokens");

  const plain = new RegExp(source, "");
  assertSameValue("abcd".replace(plain, "$2$1"), "bacd", "non-global replace swaps the first match only");
  const u = new RegExp(source, "u");
  assertSameValue("abcd".replace(u, "$2$1"), "bacd", "non-global unicode replace swaps the first match only");
}
