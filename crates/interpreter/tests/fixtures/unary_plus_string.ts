// Unary `+` on a string is JS's explicit numeric coercion — the same parse
// `Number(s)` performs. Unary `-` on a string stays rejected.
function main(): void {
  const s = "42";
  assert(+s === 42, "+string parses");
  assert(+"3.5" === 3.5, "+ on a string literal");
  assert(+"  7  " === 7, "surrounding whitespace is ignored");
  assert(+"0x10" === 16, "hex literal text");
  assert(+"" === 0, "the empty string is 0");
  assert(+"1e3" === 1000, "exponent notation");

  const nan = +"zz";
  assert(nan !== nan, "unparseable text is NaN");
  assert(+s === Number(s), "matches Number(s) exactly");

  assert(+5 === 5, "+number is still the identity");
  const neg = -3;
  assert(+neg === -3, "+ on a negative number");

  const big = 2n;
  assert(+big === 2n, "+bigint is still the identity");

  // Every string-shaped operand, not just a bare `string`.
  const tmpl = `7${8}`;
  assert(+tmpl === 78, "template literal");
  const lit: "9" = "9";
  assert(+lit === 9, "a string-literal type");
  const either: "8" | "9" = "8";
  assert(+either === 8, "a union of string-literal types");
  assert(+new Holder().text === 12, "a string-typed object field");
  assert(+supply() === 34, "a string returned from a call");
  const aliased: S = "5";
  assert(+aliased === 5, "through a type alias");

  const maybe: string | null = "6";
  if (maybe !== null) {
    assert(+maybe === 6, "after narrowing away the null");
  }

  assert(+"0b101" === 5, "binary literal text");
  assert(+"0o17" === 15, "octal literal text");
  assert(+"Infinity" === 1 / 0, "Infinity text");
  assert(+(+"3") === 3, "applying it twice");
}

type S = string;

class Holder {
  text: string = "12";
}

function supply(): string {
  return "34";
}
