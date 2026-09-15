// test262: test/built-ins/TypedArray/prototype/fill/fill-values-relative-end.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8, 0, 1),
    [8, 0, 0],
    "Fill elements from custom end position",
  );
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8, 0, -1),
    [8, 8, 0],
    "negative end sets final position to max((length + relativeEnd), 0)",
  );
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8, 0, 5),
    [8, 8, 8],
    "end position is never higher than of length",
  );
  assertBytes(
    new Uint8Array([0, 0, 0]).fill(8, 0, -4),
    [0, 0, 0],
    "end position is 0 when (len + relativeEnd) < 0",
  );
}
