// test262: test/built-ins/RegExp/S15.10.2.7_A6_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /b{2,}c/.exec("aaabbbbcccddeeeefffff");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  assertSameValue(matched, "bbbbc", "{2,} is greedy");
  assertSameValue(index, 3, "match offset");
}
