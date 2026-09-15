// test262: test/built-ins/RegExp/S15.10.2.7_A4_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /[^"]*/.exec("\"beast\"-nickname");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  assertSameValue(matched, "", "* matches empty at position 0 before the quote");
  assertSameValue(index, 0, "match offset");
}
