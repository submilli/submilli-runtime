// test262: test/built-ins/TypedArray/prototype/lastIndexOf/search-found-returns-index.js
// Instantiated at Uint8Array.

function main(): void {
  const sample: Uint8Array = new Uint8Array([42, 43, 42, 41]);
  assertSameValue(sample.lastIndexOf(42), 2, "lastIndexOf(42)");
  assertSameValue(sample.lastIndexOf(43), 1, "lastIndexOf(43)");
  assertSameValue(sample.lastIndexOf(41), 3, "lastIndexOf(41)");
  assertSameValue(sample.lastIndexOf(41, 3), 3, "lastIndexOf(41, 3)");
  assertSameValue(sample.lastIndexOf(41, 4), 3, "lastIndexOf(41, 4)");
  assertSameValue(sample.lastIndexOf(43, 1), 1, "lastIndexOf(43, 1)");
  assertSameValue(sample.lastIndexOf(43, 2), 1, "lastIndexOf(43, 2)");
  assertSameValue(sample.lastIndexOf(43, 3), 1, "lastIndexOf(43, 3)");
  assertSameValue(sample.lastIndexOf(43, 4), 1, "lastIndexOf(43, 4)");
  assertSameValue(sample.lastIndexOf(42, 0), 0, "lastIndexOf(42, 0)");
  assertSameValue(sample.lastIndexOf(42, 1), 0, "lastIndexOf(42, 1)");
  assertSameValue(sample.lastIndexOf(42, 2), 2, "lastIndexOf(42, 2)");
  assertSameValue(sample.lastIndexOf(42, 3), 2, "lastIndexOf(42, 3)");
  assertSameValue(sample.lastIndexOf(42, 4), 2, "lastIndexOf(42, 4)");
  assertSameValue(sample.lastIndexOf(42, -4), 0, "lastIndexOf(42, -4)");
  assertSameValue(sample.lastIndexOf(42, -3), 0, "lastIndexOf(42, -3)");
  assertSameValue(sample.lastIndexOf(42, -2), 2, "lastIndexOf(42, -2)");
  assertSameValue(sample.lastIndexOf(42, -1), 2, "lastIndexOf(42, -1)");
  assertSameValue(sample.lastIndexOf(43, -3), 1, "lastIndexOf(43, -3)");
  assertSameValue(sample.lastIndexOf(43, -2), 1, "lastIndexOf(43, -2)");
  assertSameValue(sample.lastIndexOf(43, -1), 1, "lastIndexOf(43, -1)");
  assertSameValue(sample.lastIndexOf(41, -1), 3, "lastIndexOf(41, -1)");
}
