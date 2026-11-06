// test262: test/built-ins/RegExp/S15.10.2.7_A3_T1.js

function main(): void {
  const m = /\s+java\s+/.exec("language  java\n");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  assertSameValue(matched, "  java\n", "+ is greedy over whitespace");
  assertSameValue(index, 8, "match offset");
}
