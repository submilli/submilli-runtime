// `localeCompare` orders by the Unicode root collation, as `Intl.Collator`
// does for the root or an English locale, and returns -1, 0 or 1. The
// relational operators still compare UTF-16 code units.

function main(): void {
  assert("a".localeCompare("B") === -1, "a letter difference outweighs case");
  assert("a".localeCompare("A") === -1, "lower case before upper case");
  assert("A".localeCompare("a") === 1, "upper case after lower case");
  assert("\u00e4".localeCompare("z") === -1, "an accented letter sorts beside its base letter");
  assert("r\u00e9sum\u00e9".localeCompare("resume") === 1, "accents break ties after the letters");
  assert("a".localeCompare("b") === -1, "plain letters");
  assert("b".localeCompare("a") === 1, "the sign flips with the operands");
  assert("same".localeCompare("same") === 0, "equal strings");
  assert("\u00e9".localeCompare("e\u0301") === 0, "canonically equivalent spellings are equal");
  assert("".localeCompare("a") === -1, "the empty string sorts first");
  assert("a" > "B", "relational operators still compare code units");

  const names: string[] = ["Zo\u00eb", "zoe", "\u00c9mile", "emile", "adam", "Adam", "\u00c5ngstr\u00f6m", "Bob", "bob", "\u00d6zil", "oscar", "\u00c7elik", "carl"];
  names.sort((x, y) => x.localeCompare(y));
  assert(
    names.join(",") === "adam,Adam,\u00c5ngstr\u00f6m,bob,Bob,carl,\u00c7elik,emile,\u00c9mile,oscar,\u00d6zil,zoe,Zo\u00eb",
    "a sort of mixed-case and accented names matches Node",
  );
}
