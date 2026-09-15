// test262: test/built-ins/RegExp/S15.10.2.7_A5_T1.js
// RegExpExecArray shape adapted to RegExpMatch (.match/.index/.input/.groups).

function main(): void {
  const m = /java(script)?/.exec("state: javascript is extension of ecma script");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const captures = m.groups.length;
  const g1 = m.groups[0];
  assertSameValue(matched, "javascript", "greedy ? takes the optional group");
  assertSameValue(index, 7, "match offset");
  assertSameValue(captures, 1, "one capture group");
  assertSameValue(g1, "script", "capture 1");
}
