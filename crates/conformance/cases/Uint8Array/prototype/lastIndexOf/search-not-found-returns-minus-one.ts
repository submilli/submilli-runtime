// test262: test/built-ins/TypedArray/prototype/lastIndexOf/search-not-found-returns-minus-one.js
// Instantiated at Uint8Array.

function main(): void {
  const sample: Uint8Array = new Uint8Array([42, 43, 42, 41]);
  assertSameValue(sample.lastIndexOf(44), -1, "lastIndexOf(44)");
  assertSameValue(sample.lastIndexOf(44, -4), -1, "lastIndexOf(44, -4)");
  assertSameValue(sample.lastIndexOf(44, -5), -1, "lastIndexOf(44, -5)");
  assertSameValue(sample.lastIndexOf(42, -5), -1, "lastIndexOf(42, -5)");
  assertSameValue(sample.lastIndexOf(43, -4), -1, "lastIndexOf(43, -4)");
  assertSameValue(sample.lastIndexOf(43, -5), -1, "lastIndexOf(43, -5)");
  assertSameValue(sample.lastIndexOf(41, 0), -1, "lastIndexOf(41, 0)");
  assertSameValue(sample.lastIndexOf(41, 1), -1, "lastIndexOf(41, 1)");
  assertSameValue(sample.lastIndexOf(41, 2), -1, "lastIndexOf(41, 2)");
  assertSameValue(sample.lastIndexOf(43, 0), -1, "lastIndexOf(43, 0)");
}
