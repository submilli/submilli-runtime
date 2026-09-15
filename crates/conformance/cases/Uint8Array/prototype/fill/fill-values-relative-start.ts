// test262: test/built-ins/TypedArray/prototype/fill/fill-values-relative-start.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8, 1),
    [0, 8, 8],
    "Fill elements from custom start position",
  );
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8, 4),
    [0, 0, 0],
    "start position is never higher than length",
  );
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8, -1),
    [0, 0, 8],
    "start < 0 sets initial position to max((len + relativeStart), 0)",
  );
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8, -5),
    [8, 8, 8],
    "start position is 0 when (len + relativeStart) < 0",
  );
}
