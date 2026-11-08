// `s[i]` reads one UTF-16 code unit as a string, like `charAt`, and throws a
// catchable `MissingValueError` out of range, as an array read does.
function rejects(read: () => string, expected: string): boolean {
  try {
    read();
    return false;
  } catch (e) {
    return e instanceof MissingValueError && e.message === expected;
  }
}

function main(): void {
  const s: string = "héllo";
  assert(s[0] === "h", "first unit");
  assert(s[1] === "é", "a non-ASCII unit");
  assert(s[4] === "o", "last unit");
  assert(s[-0] === "h", "-0 reads unit 0");
  const i = 2;
  const c: string = s[i];
  assert(c === "l", "a computed index");
  assert("\u{1F600}"[0].length === 1, "half of a surrogate pair");

  const pair: "ab" | "cd" = "cd";
  assert(pair[1] === "d", "a union of string literals");

  const maybe: string | undefined = s.length > 0 ? s : undefined;
  assert(maybe?.[1] === "é", "an optional chain index");

  assert(rejects(() => s[5], "index 5 is out of range for `s` (length 5); use `s[5] ?? …` or `s.at(5)`"), "past the end");
  assert(rejects(() => s[-1], "index -1 is out of range for `s` (length 5); use `s[-1] ?? …` for a default, or `s.at(-1)` to count from the end"), "negative");
  assert(rejects(() => s[1.5], "index 1.5 is out of range for `s` (length 5); use `s[1.5] ?? …` for a default, or `s.at(1.5)` to count from the end"), "fractional");
  assert(rejects(() => s[NaN], "index NaN is out of range for `s` (length 5); use `s[NaN] ?? …` for a default, or `s.at(NaN)` to count from the end"), "NaN");
  assert((s[9] ?? "none") === "none", "a checked read past the end");
  assert(rejects(() => ""[0], "index 0 is out of range (length 0); use `?? …` at the read, or `.at(0)`"), "the empty string");
}
