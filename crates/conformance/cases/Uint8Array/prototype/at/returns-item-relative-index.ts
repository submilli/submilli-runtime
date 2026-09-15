// test262: test/built-ins/TypedArray/prototype/at/returns-item-relative-index.js
// Instantiated at Uint8Array; the `typeof TA.prototype.at` shape checks are
// dropped (no prototype objects).

function main(): void {
  const a: Uint8Array = new Uint8Array([1, 2, 3, 4, 5]);
  assertSameValue(a.at(0), 1, "a.at(0) must return 1");
  assertSameValue(a.at(-1), 5, "a.at(-1) must return 5");
  assertSameValue(a.at(-2), 4, "a.at(-2) must return 4");
  assertSameValue(a.at(-3), 3, "a.at(-3) must return 3");
  assertSameValue(a.at(-4), 2, "a.at(-4) must return 2");
}
