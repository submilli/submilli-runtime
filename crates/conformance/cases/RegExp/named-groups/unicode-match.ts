// test262: test/built-ins/RegExp/named-groups/unicode-match.js
// The `$`-named-group
// rows (engine-rejected, pinned by non-unicode-match) and the \k backreference
// rows (rejected by design, docs/regex.md) are dropped.

function main(): void {
  const m1 = "bab".match(/(?<a>a)/u);
  assert(m1 !== null, "(?<a>a) matches");
  if (m1 === null) {
    return;
  }
  const m1t = m1[0];
  const m1g = m1[1];
  const m1ng = m1.groups;
  assert(m1ng !== undefined, "named groups must exist");
  if (m1ng === undefined) {
    return;
  }
  const m1n = m1ng["a"];
  assertSameValue(m1t, "a", "match text");
  assertSameValue(m1g, "a", "capture 1");
  assertSameValue(m1n, "a", "named capture a");

  const m2 = "bab".match(/.(?<a>a)(?<b>.)/u);
  assert(m2 !== null, "two named groups match");
  if (m2 === null) {
    return;
  }
  const m2t = m2[0];
  const m2ng = m2.groups;
  assert(m2ng !== undefined, "named groups must exist");
  if (m2ng === undefined) {
    return;
  }
  const m2a = m2ng["a"];
  const m2b = m2ng["b"];
  assertSameValue(m2t, "bab", "match text");
  assertSameValue(m2a, "a", "named capture a");
  assertSameValue(m2b, "b", "named capture b");

  const m3 = "bab".match(/.(?<a>\w\w)/u);
  assert(m3 !== null, "named \\w\\w group matches");
  if (m3 === null) {
    return;
  }
  const m3t = m3[0];
  const m3ng = m3.groups;
  assert(m3ng !== undefined, "named groups must exist");
  if (m3ng === undefined) {
    return;
  }
  const m3a = m3ng["a"];
  assertSameValue(m3t, "bab", "match text");
  assertSameValue(m3a, "ab", "named capture a");

  const m4 = "bab".match(/(?<a>\w\w)(?<b>\w)/u);
  assert(m4 !== null, "adjacent named groups match");
  if (m4 === null) {
    return;
  }
  const m4ng = m4.groups;
  assert(m4ng !== undefined, "named groups must exist");
  if (m4ng === undefined) {
    return;
  }
  const m4a = m4ng["a"];
  const m4b = m4ng["b"];
  assertSameValue(m4a, "ba", "named capture a");
  assertSameValue(m4b, "b", "named capture b");

  const lt = "<a".match(/(?<lt><)a/u);
  assert(lt !== null, "(?<lt><) matches");
  if (lt === null) {
    return;
  }
  const ltng = lt.groups;
  assert(ltng !== undefined, "named groups must exist");
  if (ltng === undefined) {
    return;
  }
  const ltg = ltng["lt"];
  assertSameValue(ltg, "<", "named capture lt");

  const gt = ">a".match(/(?<gt>>)a/u);
  assert(gt !== null, "(?<gt>>) matches");
  if (gt === null) {
    return;
  }
  const gtng = gt.groups;
  assert(gtng !== undefined, "named groups must exist");
  if (gtng === undefined) {
    return;
  }
  const gtg = gtng["gt"];
  assertSameValue(gtg, ">", "named capture gt");
}
