// test262: test/built-ins/TypedArray/prototype/indexOf/search-not-found-returns-minus-one.js
// Instantiated at Uint8Array.

function main(): void {
  const sample: Uint8Array = new Uint8Array([42, 43, 42, 41]);
  assertSameValue(sample.indexOf(44), -1, "indexOf(44)");
  assertSameValue(sample.indexOf(43, 2), -1, "indexOf(43, 2)");
  assertSameValue(sample.indexOf(42, 3), -1, "indexOf(42, 3)");
  assertSameValue(sample.indexOf(44, -4), -1, "indexOf(44, -4)");
  assertSameValue(sample.indexOf(44, -5), -1, "indexOf(44, -5)");
  assertSameValue(sample.indexOf(42, -1), -1, "indexOf(42, -1)");
}
