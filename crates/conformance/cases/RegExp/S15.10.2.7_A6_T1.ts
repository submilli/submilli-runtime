// test262: test/built-ins/RegExp/S15.10.2.7_A6_T1.js

function main(): void {
  const m = /b{2,}c/.exec("aaabbbbcccddeeeefffff");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  assertSameValue(matched, "bbbbc", "{2,} is greedy");
  assertSameValue(index, 3, "match offset");
}
