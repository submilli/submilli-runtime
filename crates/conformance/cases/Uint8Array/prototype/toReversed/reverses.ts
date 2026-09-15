// test262: test/built-ins/TypedArray/prototype/toReversed/reverses.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(new Uint8Array([]).toReversed(), [], "empty");
  assertBytes(new Uint8Array([1]).toReversed(), [1], "single element");
  assertBytes(new Uint8Array([1, 2, 3, 4]).toReversed(), [4, 3, 2, 1], "multiple elements");
}
