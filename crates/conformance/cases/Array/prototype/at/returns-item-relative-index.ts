// test262: test/built-ins/Array/prototype/at/returns-item-relative-index.js
// Adapted: dense array; out-of-range yields null (no undefined).

function main(): void {
  const a = [1, 2, 3, 4, 5];

  assertSameValue(a.at(0), 1, "a.at(0) must return 1");
  assertSameValue(a.at(-1), 5, "a.at(-1) must return 5");
  assertSameValue(a.at(-2), 4, "a.at(-2) must return 4");
  assertSameValue(a.at(-3), 3, "a.at(-3) must return 3");
  assertSameValue(a.at(-4), 2, "a.at(-4) must return 2");
  assertSameValue(a.at(-6), null, "a.at(-6) before the start returns null");
}
