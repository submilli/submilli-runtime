// test262: test/built-ins/RegExp/named-groups/groups-object-unmatched.js
// The groups object becomes the namedGroups Map; an unmatched named capture is
// represented by undefined in the Map.

function main(): void {
  const m = /(?<a>a).|(?<x>x)/.exec("ab");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const g1 = m.groups[0];
  const g2 = m.groups[1];
  const named = m.namedGroups;
  assertSameValue(matched, "ab", "full match");
  assertSameValue(index, 0, "match offset");
  assertSameValue(g1, "a", "capture 1");
  assertSameValue(g2, undefined, "capture 2 did not participate");
  assertSameValue(named.get("a"), "a", "named capture a");
  assertSameValue(named.get("x"), undefined, "unmatched named capture x is undefined");
}
