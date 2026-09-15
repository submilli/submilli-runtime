// test262: test/built-ins/TypedArray/prototype/sort/sorted-values.js
// Uint8Array subset: the float/negative/Infinity/NaN blocks target other
// element types and are recorded in skips/Uint8Array.md. The int-array
// [1, 0, -0, 2] block keeps its expectation (-0 stores as 0).

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(new Uint8Array([4, 3, 2, 1]).sort(), [1, 2, 3, 4], "descending values");
  assertBytes(new Uint8Array([3, 4, 1, 2]).sort(), [1, 2, 3, 4], "mixed numbers");
  assertBytes(
    new Uint8Array([3, 4, 3, 1, 0, 1, 2]).sort(),
    [0, 1, 1, 2, 3, 3, 4],
    "repeating numbers",
  );
  assertBytes(new Uint8Array([1, 0, -0, 2]).sort(), [0, 0, 1, 2], "0s");
}
