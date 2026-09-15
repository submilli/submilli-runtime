// test262: test/built-ins/Set/prototype/add/will-not-add-duplicate-entry-normalizes-zero.js

function main(): void {
  const s = new Set([-0]);

  assertSameValue(s.size, 1, "The value of `s.size` is `1`");

  s.add(-0);

  assertSameValue(s.size, 1, "The value of `s.size` is `1`, after executing `s.add(-0)`");

  s.add(0);

  assertSameValue(s.size, 1, "The value of `s.size` is `1`, after executing `s.add(0)`");
}
