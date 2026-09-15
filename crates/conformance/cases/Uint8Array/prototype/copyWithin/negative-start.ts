// test262: test/built-ins/TypedArray/prototype/copyWithin/negative-start.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(
    new Uint8Array([0, 1, 2, 3]).copyWithin(0, -1),
    [3, 1, 2, 3],
    "[0, 1, 2, 3].copyWithin(0, -1) -> [3, 1, 2, 3]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3, 4]).copyWithin(2, -2),
    [0, 1, 3, 4, 4],
    "[0, 1, 2, 3, 4].copyWithin(2, -2) -> [0, 1, 3, 4, 4]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3, 4]).copyWithin(1, -2),
    [0, 3, 4, 3, 4],
    "[0, 1, 2, 3, 4].copyWithin(1, -2) -> [0, 3, 4, 3, 4]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3]).copyWithin(-1, -2),
    [0, 1, 2, 2],
    "[0, 1, 2, 3].copyWithin(-1, -2) -> [0, 1, 2, 2]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3, 4]).copyWithin(-2, -3),
    [0, 1, 2, 2, 3],
    "[0, 1, 2, 3, 4].copyWithin(-2, -3) -> [0, 1, 2, 2, 3]",
  );
  assertBytes(
    new Uint8Array([0, 1, 2, 3, 4]).copyWithin(-5, -2),
    [3, 4, 2, 3, 4],
    "[0, 1, 2, 3, 4].copyWithin(-5, -2) -> [3, 4, 2, 3, 4]",
  );
}
