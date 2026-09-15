// test262: test/built-ins/Uint8Array/fromBase64/last-chunk-handling.js
// Default (loose) handling must accept non-zero padding bits in the final
// chunk and discard them.
// expect-fail: fromBase64 rejects a final chunk with non-zero padding bits ('ZXhhZh==' / 'ZXhhZh'); the standard's default loose handling discards the extra bits

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  // non-zero padding bits
  assertBytes(Uint8Array.fromBase64("ZXhhZh=="), [101, 120, 97, 102], "padded, non-zero padding bits");

  // non-zero padding bits, no padding
  assertBytes(Uint8Array.fromBase64("ZXhhZh"), [101, 120, 97, 102], "unpadded, non-zero padding bits");
}
