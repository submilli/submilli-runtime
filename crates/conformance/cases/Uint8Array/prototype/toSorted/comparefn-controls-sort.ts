// test262: test/built-ins/TypedArray/prototype/toSorted/comparefn-controls-sort.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function reverseNumericCompare(a: number, b: number): number {
  return b - a;
}

function main(): void {
  assertBytes(
    new Uint8Array([1, 2, 3, 4]).toSorted(reverseNumericCompare),
    [4, 3, 2, 1],
    "ascending input",
  );
  assertBytes(
    new Uint8Array([4, 3, 2, 1]).toSorted(reverseNumericCompare),
    [4, 3, 2, 1],
    "descending input",
  );
  assertBytes(
    new Uint8Array([33, 3, 22, 2, 111, 11, 1]).toSorted(reverseNumericCompare),
    [111, 33, 22, 11, 3, 2, 1],
    "mixed input",
  );
}
