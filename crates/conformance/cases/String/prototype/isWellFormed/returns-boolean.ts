// test262: test/built-ins/String/prototype/isWellFormed/returns-boolean.js
// The `typeof String.prototype.isWellFormed` reflection check is dropped;
// lone surrogates are built with fromCharCode — the lexer rejects unpaired
// surrogate escapes.

function main(): void {
  const leadingPoo = String.fromCharCode(0xd83d);
  const trailingPoo = String.fromCharCode(0xdca9);
  const wholePoo = leadingPoo + trailingPoo;

  assertSameValue(
    ("a" + leadingPoo + "c" + leadingPoo + "e").isWellFormed(),
    false,
    "lone leading surrogates are not well-formed",
  );
  assertSameValue(
    ("a" + trailingPoo + "c" + trailingPoo + "e").isWellFormed(),
    false,
    "lone trailing surrogates are not well-formed",
  );
  assertSameValue(
    ("a" + trailingPoo + leadingPoo + "d").isWellFormed(),
    false,
    "a wrong-ordered surrogate pair is not well-formed",
  );

  assertSameValue("a💩c".isWellFormed(), true, "a surrogate pair using a literal code point is well-formed");
  assertSameValue("a💩c".isWellFormed(), true, "a surrogate pair formed by escape sequences is well-formed");
  assertSameValue(("a" + leadingPoo + trailingPoo + "d").isWellFormed(), true, "a surrogate pair formed by concatenation is well-formed");
  assertSameValue(wholePoo.slice(0, 1).isWellFormed(), false, "a surrogate pair sliced to the leading surrogate is not well-formed");
  assertSameValue(wholePoo.slice(1).isWellFormed(), false, "a surrogate pair sliced to the trailing surrogate is not well-formed");
  assertSameValue("abc".isWellFormed(), true, "a latin-1 string is well-formed");
  assertSameValue("a▨c".isWellFormed(), true, "a non-ASCII character is well-formed");
}
