// test262: test/built-ins/RegExp/S15.10.2.7_A3_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /\s+java\s+/.exec("language  java\n");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  assertSameValue(matched, "  java\n", "+ is greedy over whitespace");
  assertSameValue(index, 8, "match offset");
}
