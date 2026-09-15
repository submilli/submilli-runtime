// test262: test/built-ins/Array/prototype/at/returns-item.js
// Adapted: dense array (sparse-hole element rejected by design); the hole's
// `undefined` read is covered by the rejected original.

function main(): void {
  const a = [1, 2, 3, 4, 5];

  assertSameValue(a.at(0), 1, "a.at(0) must return 1");
  assertSameValue(a.at(1), 2, "a.at(1) must return 2");
  assertSameValue(a.at(2), 3, "a.at(2) must return 3");
  assertSameValue(a.at(3), 4, "a.at(3) must return 4");
  assertSameValue(a.at(4), 5, "a.at(4) must return 5");
  assertSameValue(a.at(5), null, "a.at(5) past the end returns null");
}
