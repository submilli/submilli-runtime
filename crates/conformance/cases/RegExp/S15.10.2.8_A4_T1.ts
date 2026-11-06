// test262: test/built-ins/RegExp/S15.10.2.8_A4_T1.js

function main(): void {
  const m = /ab.de/.exec("abcde");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  assertSameValue(matched, "abcde", ". matches any non-line-terminator");
  assertSameValue(index, 0, "match offset");
}
