// test262: test/built-ins/Uint8Array/fromBase64/last-chunk-handling.js
// Default-mode subset: our Base64Options has no lastChunkHandling — the
// option-bearing variants are recorded in skips/Uint8Array.md. The
// trailing-bits rows live in last-chunk-handling-trailing-bits.ts.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  // padding
  assertBytes(Uint8Array.fromBase64("ZXhhZg=="), [101, 120, 97, 102], "padded");

  // no padding
  assertBytes(Uint8Array.fromBase64("ZXhhZg"), [101, 120, 97, 102], "unpadded");

  // partial padding
  assertThrows((): void => {
    Uint8Array.fromBase64("ZXhhZg=");
  }, "partial padding");

  // excess padding
  assertThrows((): void => {
    Uint8Array.fromBase64("ZXhhZg===");
  }, "excess padding");
}
