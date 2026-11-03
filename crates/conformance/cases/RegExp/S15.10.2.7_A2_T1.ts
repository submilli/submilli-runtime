// test262: test/built-ins/RegExp/S15.10.2.7_A2_T1.js

function main(): void {
  const m = /\w{3}\d?/.exec("CE\uFFFFL\uFFDDbox127");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const input = m.input;
  assertSameValue(matched, "box1", "{3} then optional digit");
  assertSameValue(index, 5, "match offset in code units");
  assertSameValue(input, "CE\uFFFFL\uFFDDbox127", "input round-trips");
}
