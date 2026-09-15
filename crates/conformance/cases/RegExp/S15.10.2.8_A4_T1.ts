// test262: test/built-ins/RegExp/S15.10.2.8_A4_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /ab.de/.exec("abcde");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  assertSameValue(matched, "abcde", ". matches any non-line-terminator");
  assertSameValue(index, 0, "match offset");
}
