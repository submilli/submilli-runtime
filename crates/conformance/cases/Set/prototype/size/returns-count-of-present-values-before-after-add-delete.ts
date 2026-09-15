// test262: test/built-ins/Set/prototype/size/returns-count-of-present-values-before-after-add-delete.js

function main(): void {
  const s = new Set<number>();

  assertSameValue(s.size, 0, "The value of `s.size` is `0`");

  s.add(0);

  assertSameValue(s.size, 1, "The value of `s.size` is `1`, after executing `s.add(0)`");

  s.delete(0);

  assertSameValue(s.size, 0, "The value of `s.size` is `0`, after executing `s.delete(0)`");
}
