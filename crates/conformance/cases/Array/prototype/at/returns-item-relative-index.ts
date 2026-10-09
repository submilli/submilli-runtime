// test262: test/built-ins/Array/prototype/at/returns-item-relative-index.js
// Adapted: the sparse hole at index 4 is an explicit undefined element (sparse
// arrays are rejected by design), so the array is typed (number | undefined)[].
// The `typeof Array.prototype.at` check is dropped (prototype objects are not
// values).

function main(): void {
  const a: (number | undefined)[] = [1, 2, 3, 4, undefined, 5];

  assertSameValue(a.at(0), 1, "a.at(0) must return 1");
  assertSameValue(a.at(-1), 5, "a.at(-1) must return 5");
  assertSameValue(a.at(-2), undefined, "a.at(-2) returns undefined");
  assertSameValue(a.at(-3), 4, "a.at(-3) must return 4");
  assertSameValue(a.at(-4), 3, "a.at(-4) must return 3");
}
