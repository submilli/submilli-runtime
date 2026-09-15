// test262: test/built-ins/Set/prototype/add/will-not-add-duplicate-entry.js

function main(): void {
  const s = new Set<number>();

  assertSameValue(s.size, 0, "The value of `s.size` is `0`");

  s.add(1);
  s.add(1);

  assertSameValue(s.size, 1, "The value of `s.size` is `1`, after executing `s.add(1); s.add(1);`");
}
