// test262: test/built-ins/Set/set-iterable.js

function main(): void {
  const s = new Set([1, 2]);

  assertSameValue(s.size, 2, "The value of `s.size` is `2`");
}
