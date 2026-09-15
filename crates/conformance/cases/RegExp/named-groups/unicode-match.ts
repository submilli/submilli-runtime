// test262: test/built-ins/RegExp/named-groups/unicode-match.js
// match() result adapted to RegExpMatch with .namedGroups; the `$`-named-group
// rows (engine-rejected, pinned by non-unicode-match) and the \k backreference
// rows (rejected by design, docs/regex.md) are dropped.

function main(): void {
  const m1 = "bab".match(/(?<a>a)/u);
  assert(m1 !== null, "(?<a>a) matches");
  if (m1 === null) {
    return;
  }
  const m1t = m1.match;
  const m1g = m1.groups[0];
  const m1ng = m1.namedGroups;
  const m1n = m1ng.get("a");
  assertSameValue(m1t, "a", "match text");
  assertSameValue(m1g, "a", "capture 1");
  assertSameValue(m1n, "a", "named capture a");

  const m2 = "bab".match(/.(?<a>a)(?<b>.)/u);
  assert(m2 !== null, "two named groups match");
  if (m2 === null) {
    return;
  }
  const m2t = m2.match;
  const m2ng = m2.namedGroups;
  const m2a = m2ng.get("a");
  const m2b = m2ng.get("b");
  assertSameValue(m2t, "bab", "match text");
  assertSameValue(m2a, "a", "named capture a");
  assertSameValue(m2b, "b", "named capture b");

  const m3 = "bab".match(/.(?<a>\w\w)/u);
  assert(m3 !== null, "named \\w\\w group matches");
  if (m3 === null) {
    return;
  }
  const m3t = m3.match;
  const m3ng = m3.namedGroups;
  const m3a = m3ng.get("a");
  assertSameValue(m3t, "bab", "match text");
  assertSameValue(m3a, "ab", "named capture a");

  const m4 = "bab".match(/(?<a>\w\w)(?<b>\w)/u);
  assert(m4 !== null, "adjacent named groups match");
  if (m4 === null) {
    return;
  }
  const m4ng = m4.namedGroups;
  const m4a = m4ng.get("a");
  const m4b = m4ng.get("b");
  assertSameValue(m4a, "ba", "named capture a");
  assertSameValue(m4b, "b", "named capture b");

  const lt = "<a".match(/(?<lt><)a/u);
  assert(lt !== null, "(?<lt><) matches");
  if (lt === null) {
    return;
  }
  const ltng = lt.namedGroups;
  const ltg = ltng.get("lt");
  assertSameValue(ltg, "<", "named capture lt");

  const gt = ">a".match(/(?<gt>>)a/u);
  assert(gt !== null, "(?<gt>>) matches");
  if (gt === null) {
    return;
  }
  const gtng = gt.namedGroups;
  const gtg = gtng.get("gt");
  assertSameValue(gtg, ">", "named capture gt");
}
