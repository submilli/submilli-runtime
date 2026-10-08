// test262: test/built-ins/RegExp/named-groups/string-replace-escaped.js

function main(): void {
  for (const flags of ["", "u"]) {
    const re = new RegExp("(?<fst>.)", flags);
    assertSameValue("abc".replace(re, "$$<fst>"), "$<fst>bc", "$$ is a literal $ before <fst>");
  }
}
