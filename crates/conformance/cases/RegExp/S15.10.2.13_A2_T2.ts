// test262: test/built-ins/RegExp/S15.10.2.13_A2_T2.js
// expect-fail: [^] (match any character) is valid in ECMA-262; the engine rejects it at compile time with "unclosed character class"

function main(): void {
  const m = /a[^]/.exec("   a\t\n");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  assertSameValue(matched, "a\t", "[^] matches any character");
  assertSameValue(index, 3, "match offset");
}
