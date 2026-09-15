// test262: test/built-ins/TypedArray/prototype/reverse/reverts.js
// Instantiated at Uint8Array. The original drives two views over one
// ArrayBuffer; with no buffer backing this keeps the even- and odd-length
// reversals on independent arrays.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  const sample: Uint8Array = new Uint8Array([42, 43, 2, 1]);
  sample.reverse();
  assertBytes(sample, [1, 2, 43, 42], "even length");

  const other: Uint8Array = new Uint8Array([7, 17, 1, 0, 42]);
  other.reverse();
  assertBytes(other, [42, 0, 1, 17, 7], "odd length");
}
