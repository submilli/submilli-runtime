// test262: test/built-ins/RegExp/S15.10.2.7_A1_T1.js

function main(): void {
  const m = /\d{2,4}/.exec("the answer is 42");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  const input = m.input;
  assertSameValue(matched, "42", "{2,4} matches the two digits");
  assertSameValue(index, 14, "match offset");
  assertSameValue(input, "the answer is 42", "input round-trips");
}
