// `codePointAt` out of range has no code point and returns `undefined`, as in
// JavaScript, so the result must be narrowed.
function main(): void {
  const s = "a😀";
  assert((s.codePointAt(1) ?? -1) === 0x1f600, "a surrogate pair decodes");
  assert((s.codePointAt(3) ?? -1) === -1, "past the end");
  assert((s.codePointAt(-1) ?? -1) === -1, "before the start");
  const cp = s.codePointAt(0);
  if (cp) {
    assert(cp + 1 === 98, "a narrowed code point is a number");
  }
}
