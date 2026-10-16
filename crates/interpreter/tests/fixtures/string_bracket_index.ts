// `s[i]` reads one UTF-16 code unit as a string, like `charAt`, and throws a
// catchable `RangeError` out of range, as an array read does.
function rejects(read: () => string): boolean {
  try {
    read();
    return false;
  } catch (e) {
    return e instanceof RangeError && e.message === "index out of range";
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

  assert(rejects(() => s[5]), "past the end");
  assert(rejects(() => s[-1]), "negative");
  assert(rejects(() => s[1.5]), "fractional");
  assert(rejects(() => s[NaN]), "NaN");
  assert(rejects(() => ""[0]), "the empty string");
}
