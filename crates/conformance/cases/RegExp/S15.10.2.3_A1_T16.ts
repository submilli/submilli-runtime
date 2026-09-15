// test262: test/built-ins/RegExp/S15.10.2.3_A1_T16.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /()|/.exec("");
  assert(m !== null, "exec must match the empty string");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const input = m.input;
  const captures = m.groups.length;
  const g1 = m.groups[0];
  assertSameValue(matched, "", "empty alternative matches empty");
  assertSameValue(index, 0, "match offset");
  assertSameValue(input, "", "input round-trips");
  assertSameValue(captures, 1, "one capture group");
  assertSameValue(g1, "", "empty group captures the empty string");
}
