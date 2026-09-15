// test262: test/built-ins/RegExp/named-groups/non-unicode-match.js
// expect-fail: `$` is a valid GroupName character in ECMA-262 ((?<$>a)); the engine rejects it at compile time with "invalid capture group character"
// match() result adapted to RegExpMatch; the \k backreference rows are dropped
// (backreferences are rejected by design, docs/regex.md).

function main(): void {
  const m1 = "bab".match(/(?<a>a)/);
  assert(m1 !== null, "(?<a>a) matches");
  if (m1 === null) {
    return;
  }
  const m1t = m1.match;
  const m1g = m1.groups[0];
  assertSameValue(m1t, "a", "match text");
  assertSameValue(m1g, "a", "capture 1");

  const m2 = "bab".match(/(?<a42>a)/);
  assert(m2 !== null, "(?<a42>a) matches");
  if (m2 === null) {
    return;
  }
  const m2t = m2.match;
  assertSameValue(m2t, "a", "digits allowed in group names");

  const m3 = "bab".match(/(?<_>a)/);
  assert(m3 !== null, "(?<_>a) matches");
  if (m3 === null) {
    return;
  }
  const m3t = m3.match;
  assertSameValue(m3t, "a", "underscore allowed in group names");

  const m4 = "bab".match(/(?<$>a)/);
  assert(m4 !== null, "(?<$>a) matches");
  if (m4 === null) {
    return;
  }
  const m4t = m4.match;
  assertSameValue(m4t, "a", "$ allowed in group names");

  const m5 = "bab".match(/.(?<a>a)(?<b>.)/);
  assert(m5 !== null, "two named groups match");
  if (m5 === null) {
    return;
  }
  const m5t = m5.match;
  const m5g1 = m5.groups[0];
  const m5g2 = m5.groups[1];
  assertSameValue(m5t, "bab", "match text");
  assertSameValue(m5g1, "a", "capture 1");
  assertSameValue(m5g2, "b", "capture 2");
}
