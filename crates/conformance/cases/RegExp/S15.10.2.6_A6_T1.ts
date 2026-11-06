// test262: test/built-ins/RegExp/S15.10.2.6_A6_T1.js

function main(): void {
  const m = /^.*?$/.exec("Hello World");
  assert(m !== null, "exec must match");
  if (m === null) {
    return;
  }
  const matched = m[0];
  const index = m.index;
  assertSameValue(matched, "Hello World", "lazy .*? runs to the end because of $");
  assertSameValue(index, 0, "match offset");
}
