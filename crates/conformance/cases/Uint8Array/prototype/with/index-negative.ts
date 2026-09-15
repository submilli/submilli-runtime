// test262: test/built-ins/TypedArray/prototype/with/index-negative.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  const arr: Uint8Array = new Uint8Array([0, 1, 2]);

  assertBytes(arr.with(-1, 4), [0, 1, 4], "with(-1, 4)");
  assertBytes(arr.with(-3, 4), [4, 1, 2], "with(-3, 4)");
  // -0 is not negative.
  assertBytes(arr.with(-0, 4), [4, 1, 2], "with(-0, 4)");
}
