// test262: test/built-ins/Set/prototype/delete/delete-entry-normalizes-zero.js

function main(): void {
  const s = new Set([-0]);

  assertSameValue(s.size, 1, "The value of `s.size` is `1`");

  const result = s.delete(0);

  assertSameValue(s.size, 0, "The value of `s.size` is `0`, after executing `s.delete(-0)`");
  assertSameValue(result, true, "The result of `s.delete(+0)` is `true`");
}
