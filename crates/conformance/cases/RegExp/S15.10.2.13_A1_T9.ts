// test262: test/built-ins/RegExp/S15.10.2.13_A1_T9.js

function main(): void {
  const m = /[1234567].{2}/.exec("abc6defghijkl");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  assertSameValue(matched, "6de", "character class then two dots");
  assertSameValue(index, 3, "match offset");
}
