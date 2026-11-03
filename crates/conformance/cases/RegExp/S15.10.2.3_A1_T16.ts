// test262: test/built-ins/RegExp/S15.10.2.3_A1_T16.js

function main(): void {
  const m = /()|/.exec("");
  assert(m !== null, "exec must match the empty string");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const input = m.input;
  const captures = m.length - 1;
  const g1 = m[1];
  assertSameValue(matched, "", "empty alternative matches empty");
  assertSameValue(index, 0, "match offset");
  assertSameValue(input, "", "input round-trips");
  assertSameValue(captures, 1, "one capture group");
  assertSameValue(g1, "", "empty group captures the empty string");
}
