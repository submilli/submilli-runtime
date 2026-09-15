// test262: test/built-ins/TypedArray/prototype/includes/search-not-found-returns-false.js
// Instantiated at Uint8Array.

function main(): void {
  const sample: Uint8Array = new Uint8Array([42, 43, 42, 41]);
  assertSameValue(sample.includes(44), false, "includes(44)");
  assertSameValue(sample.includes(43, 2), false, "includes(43, 2)");
  assertSameValue(sample.includes(42, 3), false, "includes(42, 3)");
  assertSameValue(sample.includes(44, -4), false, "includes(44, -4)");
  assertSameValue(sample.includes(44, -5), false, "includes(44, -5)");
  assertSameValue(sample.includes(42, -1), false, "includes(42, -1)");
}
