// test262: test/built-ins/RegExp/S15.10.2.7_A2_T1.js
// expect-fail: RegExpMatch.index is a UTF-8 byte offset (9 here), not the UTF-16 code-unit offset 5 ECMA-262 specifies — non-ASCII prefixes shift it
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /\w{3}\d?/.exec("CE\uFFFFL\uFFDDbox127");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const input = m.input;
  assertSameValue(matched, "box1", "{3} then optional digit");
  assertSameValue(index, 5, "match offset in code units");
  assertSameValue(input, "CE\uFFFFL\uFFDDbox127", "input round-trips");
}
