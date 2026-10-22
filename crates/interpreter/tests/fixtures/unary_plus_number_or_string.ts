// `+x` on a `number | string` converts whichever it holds, as each member alone
// does: a string parses as `Number(s)` would, and a number is unchanged.
enum Code { A = 3 }
enum Text { Twelve = "12" }

function asNumber(x: number | string): number {
  return +x;
}

function main(): void {
  assert(asNumber(" 42 ") === 42, "a string parses");
  assert(asNumber(7) === 7, "a number is unchanged");
  assert(Number.isNaN(asNumber("x")), "unparseable text is NaN");
  assert(asNumber("0x10") === 16, "hex text");
  assert(asNumber("") === 0, "empty text is 0");
  const lit: 1 | "8" = Math.random() > 2 ? 1 : "8";
  assert(+lit === 8, "a literal union");
  const e: Code | Text = Math.random() > 2 ? Code.A : Text.Twelve;
  assert(+e === 12, "an enum union");
  let x: number | string = "0";
  for (let i = 0; i < 3; i++) {
    x = +x + 1;
  }
  assert(x === 3, "the result is a number");
  console.log(asNumber("2.5"), asNumber(-1));
}
