// test262: test/built-ins/RegExp/dotall/with-dotall.js
// expect-fail: a single `.` should not match a supplementary-plane code point (it spans two code units in ECMA-262); the engine matches the whole code point
// Lone-surrogate rows are dropped (host strings cannot hold unpaired surrogates).

function main(): void {
  const astral = String.fromCodePoint(66304);
  const plain = /^.$/s;
  assert(plain.test("a"), "a");
  assert(plain.test("3"), "3");
  assert(plain.test("π"), "U+03C0");
  assert(plain.test("‧"), "U+2027");
  assert(plain.test("\u0085"), "U+0085");
  assert(plain.test("\u000B"), "U+000B");
  assert(plain.test("\u000C"), "U+000C");
  assert(plain.test("\u180E"), "U+180E");
  assert(plain.test("\n"), "LF");
  assert(plain.test("\r"), "CR");
  assert(plain.test("\u2028"), "U+2028");
  assert(plain.test("\u2029"), "U+2029");
  assert(!plain.test(astral), "supplementary plane not matched by a single .");

  const multi = /^.$/sm;
  assert(multi.test("a"), "m: a");
  assert(multi.test("\n"), "m: LF");
  assert(multi.test("\u2028"), "m: U+2028");
  assert(!multi.test(astral), "m: supplementary plane not matched by a single .");
}
