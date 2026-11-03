// test262: test/built-ins/RegExp/prototype/exec/y-init-lastindex.js
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/RegExp/prototype/exec/y-init-lastindex.js
// Fields are copied after the null check before assertion calls.

function main(): void {
  const r = /./y;
  r.lastIndex = 1;

  const match = r.exec("abc");

  assert(match !== null);
  if (match === null) {
    return;
  }
  const length = match.length;
  const first = match[0];
  assertSameValue(length, 1);
  assertSameValue(first, "b");
}
