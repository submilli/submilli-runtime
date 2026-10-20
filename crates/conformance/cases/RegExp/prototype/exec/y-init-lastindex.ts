// test262: test/built-ins/RegExp/prototype/exec/y-init-lastindex.js
// Source: https://github.com/tc39/test262/blob/d53ebc8b1931d9f1541e43d59650776e4b766248/test/built-ins/RegExp/prototype/exec/y-init-lastindex.js
// RegExpExecArray becomes RegExpMatch: length is groups.length + 1 and [0]
// is match. Fields are copied after the null check before assertion calls.

function main(): void {
  const r = /./y;
  r.lastIndex = 1;

  const match = r.exec("abc");

  assert(match !== null);
  if (match === null) {
    return;
  }
  const length = match.groups.length + 1;
  const first = match.match;
  assertSameValue(length, 1);
  assertSameValue(first, "b");
}
