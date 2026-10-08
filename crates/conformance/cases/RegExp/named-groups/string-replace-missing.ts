// test262: test/built-ins/RegExp/named-groups/string-replace-missing.js

function main(): void {
  const source = "(?<fst>.)(?<snd>.)|(?<thd>x)";

  const plain = new RegExp(source, "");
  assertSameValue("abcd".replace(plain, "$<fth>"), "cd", "missing group name replaces with the empty string");
  assertSameValue("abcd".replace(plain, "$<>"), "cd", "empty group name replaces with the empty string");

  const g = new RegExp(source, "g");
  assertSameValue("abcd".replace(g, "$<fth>"), "", "global: missing group name replaces every match with empty");
  assertSameValue("abcd".replace(g, "$<>"), "", "global: empty group name replaces every match with empty");
}
