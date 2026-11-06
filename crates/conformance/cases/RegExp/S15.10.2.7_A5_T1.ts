// test262: test/built-ins/RegExp/S15.10.2.7_A5_T1.js

function main(): void {
  const m = /java(script)?/.exec("state: javascript is extension of ecma script");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const captures = m.length - 1;
  const g1 = m[1];
  assertSameValue(matched, "javascript", "greedy ? takes the optional group");
  assertSameValue(index, 7, "match offset");
  assertSameValue(captures, 1, "one capture group");
  assertSameValue(g1, "script", "capture 1");
}
