// test262: test/built-ins/RegExp/prototype/exec/S15.10.6.2_A1_T2.js
// `new String("123")` receiver replaced by the plain string; RegExpExecArray
// shape adapted to RegExpMatch (unmatched captures are undefined).

function main(): void {
  const m = /((1)|(12))((3)|(23))/.exec("123");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m.match;
  const index = m.index;
  const input = m.input;
  const captures = m.groups.length;
  const g1 = m.groups[0];
  const g2 = m.groups[1];
  const g3 = m.groups[2];
  const g4 = m.groups[3];
  const g5 = m.groups[4];
  const g6 = m.groups[5];
  assertSameValue(matched, "123", "full match");
  assertSameValue(index, 0, "match offset");
  assertSameValue(input, "123", "input round-trips");
  assertSameValue(captures, 6, "six capture groups");
  assertSameValue(g1, "1", "capture 1");
  assertSameValue(g2, "1", "capture 2");
  assertSameValue(g3, undefined, "capture 3 did not participate");
  assertSameValue(g4, "23", "capture 4");
  assertSameValue(g5, undefined, "capture 5 did not participate");
  assertSameValue(g6, "23", "capture 6");
}
