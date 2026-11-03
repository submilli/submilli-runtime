// test262: test/built-ins/RegExp/S15.10.2.5_A1_T4.js
// expect-fail: a capture inside a quantified group should be cleared on iterations where it does not participate (ECMA-262 RepeatMatcher), so capture 4 should be undefined; the engine keeps "bbb" from an earlier iteration

function main(): void {
  const m = /(z)((a+)?(b+)?(c))*/.exec("zaacbbbcac");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const g1 = m[1];
  const g2 = m[2];
  const g3 = m[3];
  const g4 = m[4];
  const g5 = m[5];
  assertSameValue(matched, "zaacbbbcac", "full match");
  assertSameValue(index, 0, "match offset");
  assertSameValue(g1, "z", "capture 1");
  assertSameValue(g2, "ac", "capture 2 holds the last iteration");
  assertSameValue(g3, "a", "capture 3 holds the last iteration");
  assertSameValue(g4, undefined, "capture 4 did not participate in the last iteration");
  assertSameValue(g5, "c", "capture 5");
}
