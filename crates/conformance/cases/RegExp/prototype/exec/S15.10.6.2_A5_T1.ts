// test262: test/built-ins/RegExp/prototype/exec/S15.10.6.2_A5_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const re = /(?:ab|cd)\d?/g;
  const first = re.exec("aac1dz2233a1bz12nm444ab42");
  assert(first !== null, "first exec must match");
  if (first === null) {
    return;
  }
  const matched = first.match;
  const index = first.index;
  assertSameValue(matched, "ab4", "g exec finds the only ab/cd match");
  assertSameValue(index, 21, "match offset");
  assertSameValue(re.lastIndex, 24, "lastIndex advanced past the match");

  const second = re.exec("aacd22");
  assertSameValue(second === null, true, "stale lastIndex beyond the new input fails the match");
  assertSameValue(re.lastIndex, 0, "lastIndex resets to 0 after the failure");

  const third = re.exec("aacd22");
  assert(third !== null, "after the reset exec matches from 0");
  if (third === null) {
    return;
  }
  const matched3 = third.match;
  assertSameValue(matched3, "cd2", "match after reset");
}
