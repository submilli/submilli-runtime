// test262: test/built-ins/RegExp/S15.10.2.7_A4_T1.js

function main(): void {
  const m = /[^"]*/.exec("\"beast\"-nickname");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  assertSameValue(matched, "", "* matches empty at position 0 before the quote");
  assertSameValue(index, 0, "match offset");
}
