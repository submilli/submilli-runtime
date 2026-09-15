// test262: test/built-ins/TypedArray/prototype/fill/fill-values.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(new Uint8Array([]).fill(8), [], "does not fill an empty instance");
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8),
    [8, 8, 8],
    "Default start and end indexes are 0 and this.length",
  );
}
