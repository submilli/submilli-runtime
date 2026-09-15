// test262: test/built-ins/String/prototype/toWellFormed/returns-well-formed-string.js
// The `typeof String.prototype.toWellFormed` reflection check is dropped;
// lone surrogates are built with fromCharCode — the lexer rejects unpaired
// surrogate escapes.

function main(): void {
  const replacementChar = "�";
  const leadingPoo = String.fromCharCode(0xd83d);
  const trailingPoo = String.fromCharCode(0xdca9);
  const wholePoo = leadingPoo + trailingPoo;

  assertSameValue(
    ("a" + leadingPoo + "c" + leadingPoo + "e").toWellFormed(),
    "a" + replacementChar + "c" + replacementChar + "e",
    "lone leading surrogates are replaced with the expected replacement character",
  );
  assertSameValue(
    ("a" + trailingPoo + "c" + trailingPoo + "e").toWellFormed(),
    "a" + replacementChar + "c" + replacementChar + "e",
    "lone trailing surrogates are replaced with the expected replacement character",
  );
  assertSameValue(
    ("a" + trailingPoo + leadingPoo + "d").toWellFormed(),
    "a" + replacementChar + replacementChar + "d",
    "a wrong-ordered surrogate pair is replaced with two replacement characters",
  );

  assertSameValue("a💩c".toWellFormed(), "a💩c", "a surrogate pair using a literal code point is already well-formed");
  assertSameValue("a💩c".toWellFormed(), "a💩c", "a surrogate pair formed by escape sequences is already well-formed");
  assertSameValue(("a" + leadingPoo + trailingPoo + "d").toWellFormed(), "a" + wholePoo + "d", "a surrogate pair formed by concatenation is already well-formed");
  assertSameValue(wholePoo.slice(0, 1).toWellFormed(), replacementChar, "a surrogate pair sliced to the leading surrogate is replaced with the expected replacement character");
  assertSameValue(wholePoo.slice(1).toWellFormed(), replacementChar, "a surrogate pair sliced to the trailing surrogate is replaced with the expected replacement character");
  assertSameValue("abc".toWellFormed(), "abc", "a latin-1 string is already well-formed");
  assertSameValue("a▨c".toWellFormed(), "a▨c", "a string with a non-ASCII character is already well-formed");
}
