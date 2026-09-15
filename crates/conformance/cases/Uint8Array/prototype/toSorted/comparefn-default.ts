// test262: test/built-ins/TypedArray/prototype/toSorted/comparefn-default.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(new Uint8Array([4, 2, 1, 3]).toSorted(), [1, 2, 3, 4], "small values");
  assertBytes(
    new Uint8Array([111, 33, 22, 11, 3, 2, 1]).toSorted(),
    [1, 2, 3, 11, 22, 33, 111],
    "numeric order, not lexicographic",
  );
}
