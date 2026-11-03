// test262: test/built-ins/RegExp/prototype/exec/S15.10.6.2_A1_T2.js
// `new String("123")` receiver replaced by the plain string.

function main(): void {
  const m = /((1)|(12))((3)|(23))/.exec("123");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const input = m.input;
  const captures = m.length - 1;
  const g1 = m[1];
  const g2 = m[2];
  const g3 = m[3];
  const g4 = m[4];
  const g5 = m[5];
  const g6 = m[6];
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
