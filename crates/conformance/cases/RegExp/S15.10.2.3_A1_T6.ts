// test262: test/built-ins/RegExp/S15.10.2.3_A1_T6.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /ab|cd|ef/i.exec("AEKFCD");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const input = m.input;
  const captures = m.groups.length;
  assertSameValue(matched, "CD", "alternation picks the leftmost match");
  assertSameValue(index, 4, "match offset");
  assertSameValue(input, "AEKFCD", "input round-trips");
  assertSameValue(captures, 0, "no capture groups");
}
