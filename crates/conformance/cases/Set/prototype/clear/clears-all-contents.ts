// test262: test/built-ins/Set/prototype/clear/clears-all-contents.js

function main(): void {
  const s = new Set<number>();

  s.add(1).add(2).add(3);

  assertSameValue(s.size, 3, "The value of `s.size` is `3`");

  assertSameValue(s.clear(), undefined, "clear returns undefined");

  assertSameValue(s.size, 0, "The value of `s.size` is `0`, after executing `s.clear()`");
  assertSameValue(s.has(1), false, "`s.has(1)` returns `false`");
  assertSameValue(s.has(2), false, "`s.has(2)` returns `false`");
  assertSameValue(s.has(3), false, "`s.has(3)` returns `false`");
}
