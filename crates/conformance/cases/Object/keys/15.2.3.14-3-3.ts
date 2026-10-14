// test262: test/built-ins/Object/keys/15.2.3.14-3-3.js

function main(): void {
  const o = [1, 2];
  const a = Object.keys(o);

  assertSameValue(a.length, 2, "a.length");
  assertSameValue(a[0], "0", "a[0]");
  assertSameValue(a[1], "1", "a[1]");
}
