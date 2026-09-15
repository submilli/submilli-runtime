// test262: test/built-ins/RegExp/dotall/without-dotall.js
// expect-fail: without the s flag `.` should exclude every LineTerminator (CR, U+2028, U+2029) and span at most one code unit; the engine's `.` excludes only LF and matches a whole astral code point
// Lone-surrogate rows are dropped (host strings cannot hold unpaired surrogates).

function main(): void {
  const astral = String.fromCodePoint(66304);
  const plain = /^.$/;
  assert(plain.test("a"), "a");
  assert(plain.test("3"), "3");
  assert(plain.test("π"), "U+03C0");
  assert(plain.test("‧"), "U+2027");
  assert(plain.test("\u0085"), "U+0085");
  assert(plain.test("\u000B"), "U+000B");
  assert(plain.test("\u000C"), "U+000C");
  assert(plain.test("\u180E"), "U+180E");
  assert(!plain.test(astral), "supplementary plane matched by a single .");
  assert(!plain.test("\n"), "LF excluded");
  assert(!plain.test("\r"), "CR excluded");
  assert(!plain.test("\u2028"), "U+2028 excluded");
  assert(!plain.test("\u2029"), "U+2029 excluded");

  const multi = /^.$/m;
  assert(multi.test("a"), "m: a");
  assert(!multi.test("\n"), "m: LF excluded");
  assert(!multi.test("\u2028"), "m: U+2028 excluded");
}
