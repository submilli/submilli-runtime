function main(): void {
  assert("aba".replaceAll("a", "$`-$&-$'") === "-a-babab-a-", "literal prefix/suffix expansion");
  assert("abc".replaceAll("", "x") === "xaxbxcx", "empty literal replacement");
  assert("aa".replaceAll(/(a)/g, "[$1]") === "[a][a]", "regex numbered capture");
  assert("aa".replaceAll(/a/g, "$$") === "$$", "regex escaped dollar");
  assert("ab".replaceAll(/a/g, "${${${") === "${${${b", "unterminated braced captures remain literal");
  assert("a,b,c,d".split(",", 2).join("|") === "a|b", "literal limit drops suffix");
  assert("a,b,c,d".split(/,/, 2).join("|") === "a|b", "regex limit drops suffix");
  assert("abc".split("", 0).length === 0, "zero split limit");
  const lone = String.fromCharCode(0xD800);
  assert(("a"+lone+"b").replaceAll("a", "x") === "x"+lone+"b", "literal replacement preserves UTF-16");
  assert((lone+"a").split("", 2)[0] === lone, "literal split preserves UTF-16");
  for (let count = 128; count <= 256; count *= 2) {
    const input = "a,".repeat(count);
    assert(input.replaceAll("a", "bb").length === count * 3, "linear literal output");
    assert(input.replaceAll(/a/g, "bb").length === count * 3, "linear regex output");
    assert(input.split(",").length === count + 1, "incremental literal parts");
    assert(input.split(/,/).length === count + 1, "incremental regex parts");
  }
}
