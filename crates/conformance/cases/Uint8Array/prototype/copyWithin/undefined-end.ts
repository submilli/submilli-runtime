// test262: test/built-ins/TypedArray/prototype/copyWithin/undefined-end.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(
    new Uint8Array([0, 1, 2, 3]).copyWithin(0, 1, undefined),
    [1, 2, 3, 3],
    "[0, 1, 2, 3].copyWithin(0, 1, undefined) -> [1, 2, 3]",
  );

  assertBytes(
    new Uint8Array([0, 1, 2, 3]).copyWithin(0, 1),
    [1, 2, 3, 3],
    "[0, 1, 2, 3].copyWithin(0, 1) -> [1, 2, 3, 3]",
  );
}
