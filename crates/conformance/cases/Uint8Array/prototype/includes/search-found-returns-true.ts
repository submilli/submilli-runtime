// test262: test/built-ins/TypedArray/prototype/includes/search-found-returns-true.js
// Instantiated at Uint8Array.

function main(): void {
  const sample: Uint8Array = new Uint8Array([42, 43, 42, 41]);
  assertSameValue(sample.includes(42), true, "includes(42)");
  assertSameValue(sample.includes(43), true, "includes(43)");
  assertSameValue(sample.includes(43, 1), true, "includes(43, 1)");
  assertSameValue(sample.includes(42, 1), true, "includes(42, 1)");
  assertSameValue(sample.includes(42, 2), true, "includes(42, 2)");

  assertSameValue(sample.includes(42, -4), true, "includes(42, -4)");
  assertSameValue(sample.includes(42, -3), true, "includes(42, -3)");
  assertSameValue(sample.includes(42, -2), true, "includes(42, -2)");
  assertSameValue(sample.includes(42, -5), true, "includes(42, -5)");
}
