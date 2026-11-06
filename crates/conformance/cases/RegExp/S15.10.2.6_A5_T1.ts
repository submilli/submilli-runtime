// test262: test/built-ins/RegExp/S15.10.2.6_A5_T1.js

function main(): void {
  const m = /^^^^^^^robot$$$$/.exec("robot");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  assertSameValue(matched, "robot", "repeated assertions behave like a single one");
  assertSameValue(index, 0, "match offset");
}
