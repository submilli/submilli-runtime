// test262: test/built-ins/TypedArray/prototype/copyWithin/non-negative-target-start-and-end.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(
    new Uint8Array([0, 1, 2, 3]).copyWithin(0, 0, 0),
    [0, 1, 2, 3],
    "[0, 1, 2, 3].copyWithin(0, 0, 0) -> [0, 1, 2, 3]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3]).copyWithin(0, 0, 2),
    [0, 1, 2, 3],
    "[0, 1, 2, 3].copyWithin(0, 0, 2) -> [0, 1, 2, 3]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3]).copyWithin(0, 1, 2),
    [1, 1, 2, 3],
    "[0, 1, 2, 3].copyWithin(0, 1, 2) -> [1, 1, 2, 3]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3]).copyWithin(1, 0, 2),
    [0, 0, 1, 3],
    "[0, 1, 2, 3].copyWithin(1, 0, 2) -> [0, 0, 1, 3]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3, 4, 5]).copyWithin(1, 3, 5),
    [0, 3, 4, 3, 4, 5],
    "[0, 1, 2, 3, 4, 5].copyWithin(1, 3, 5) -> [0, 3, 4, 3, 4, 5]",
  );
}
