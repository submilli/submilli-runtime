// test262: test/built-ins/RegExp/S15.10.2.13_A3_T1.js
// expect-fail: [\b] (backspace inside a character class) is valid in ECMA-262; the engine rejects it at compile time with "invalid escape sequence found in character class"

function main(): void {
  const m = /.[\b]./.exec("abc\bdef");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  assertSameValue(matched, "c\bd", "\\b inside a class is backspace U+0008");
  assertSameValue(index, 2, "match offset");
}
