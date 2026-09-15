// test262: test/built-ins/Uint8Array/fromBase64/results.js

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  // standard test vectors from https://datatracker.ietf.org/doc/html/rfc4648#section-10
  assertBytes(Uint8Array.fromBase64(""), [], "decoding ''");
  assertBytes(Uint8Array.fromBase64("Zg=="), [102], "decoding 'Zg=='");
  assertBytes(Uint8Array.fromBase64("Zm8="), [102, 111], "decoding 'Zm8='");
  assertBytes(Uint8Array.fromBase64("Zm9v"), [102, 111, 111], "decoding 'Zm9v'");
  assertBytes(Uint8Array.fromBase64("Zm9vYg=="), [102, 111, 111, 98], "decoding 'Zm9vYg=='");
  assertBytes(Uint8Array.fromBase64("Zm9vYmE="), [102, 111, 111, 98, 97], "decoding 'Zm9vYmE='");
  assertBytes(Uint8Array.fromBase64("Zm9vYmFy"), [102, 111, 111, 98, 97, 114], "decoding 'Zm9vYmFy'");
}
