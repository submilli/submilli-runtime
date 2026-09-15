// test262: test/built-ins/RegExp/S15.10.2.5_A1_T4.js
// expect-fail: a capture inside a quantified group should be cleared on iterations where it does not participate (ECMA-262 RepeatMatcher), so capture 4 should be null; the engine keeps "bbb" from an earlier iteration
// RegExpExecArray shape adapted to RegExpMatch (unmatched captures are null, not undefined).

function main(): void {
  const m = /(z)((a+)?(b+)?(c))*/.exec("zaacbbbcac");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const g1 = m.groups[0];
  const g2 = m.groups[1];
  const g3 = m.groups[2];
  const g4 = m.groups[3];
  const g5 = m.groups[4];
  assertSameValue(matched, "zaacbbbcac", "full match");
  assertSameValue(index, 0, "match offset");
  assertSameValue(g1, "z", "capture 1");
  assertSameValue(g2, "ac", "capture 2 holds the last iteration");
  assertSameValue(g3, "a", "capture 3 holds the last iteration");
  assertSameValue(g4, null, "capture 4 did not participate in the last iteration");
  assertSameValue(g5, "c", "capture 5");
}
