// test262: test/built-ins/RegExp/S15.10.2.6_A5_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /^^^^^^^robot$$$$/.exec("robot");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  assertSameValue(matched, "robot", "repeated assertions behave like a single one");
  assertSameValue(index, 0, "match offset");
}
