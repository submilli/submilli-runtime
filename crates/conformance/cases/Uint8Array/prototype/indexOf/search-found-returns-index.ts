// test262: test/built-ins/TypedArray/prototype/indexOf/search-found-returns-index.js
// Instantiated at Uint8Array.

function main(): void {
  const sample: Uint8Array = new Uint8Array([42, 43, 42, 41]);
  assertSameValue(sample.indexOf(42), 0, "indexOf(42)");
  assertSameValue(sample.indexOf(43), 1, "indexOf(43)");
  assertSameValue(sample.indexOf(43, 1), 1, "indexOf(43, 1)");
  assertSameValue(sample.indexOf(42, 1), 2, "indexOf(42, 1)");
  assertSameValue(sample.indexOf(42, 2), 2, "indexOf(42, 2)");

  assertSameValue(sample.indexOf(42, -4), 0, "indexOf(42, -4)");
  assertSameValue(sample.indexOf(42, -3), 2, "indexOf(42, -3)");
  assertSameValue(sample.indexOf(42, -2), 2, "indexOf(42, -2)");
  assertSameValue(sample.indexOf(42, -5), 0, "indexOf(42, -5)");
}
