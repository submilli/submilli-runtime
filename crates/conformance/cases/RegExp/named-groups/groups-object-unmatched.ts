// test262: test/built-ins/RegExp/named-groups/groups-object-unmatched.js

function main(): void {
  const m = /(?<a>a).|(?<x>x)/.exec("ab");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const g1 = m[1];
  const g2 = m[2];
  const named = m.groups;
  assert(named !== undefined, "named groups must exist");
  if (named === undefined) {
    return;
  }
  assertSameValue(matched, "ab", "full match");
  assertSameValue(index, 0, "match offset");
  assertSameValue(g1, "a", "capture 1");
  assert(g2 === undefined, "capture 2 did not participate");
  assertSameValue(named["a"], "a", "named capture a");
  assert(named["x"] === undefined, "unmatched named capture x is undefined");
}
