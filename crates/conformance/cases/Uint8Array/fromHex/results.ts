// test262: test/built-ins/Uint8Array/fromHex/results.js

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  assertBytes(Uint8Array.fromHex(""), [], "decoding ''");
  assertBytes(Uint8Array.fromHex("66"), [102], "decoding '66'");
  assertBytes(Uint8Array.fromHex("666f"), [102, 111], "decoding '666f'");
  assertBytes(Uint8Array.fromHex("666F"), [102, 111], "decoding '666F'");
  assertBytes(Uint8Array.fromHex("666f6f"), [102, 111, 111], "decoding '666f6f'");
  assertBytes(Uint8Array.fromHex("666F6f"), [102, 111, 111], "decoding '666F6f'");
  assertBytes(Uint8Array.fromHex("666f6f62"), [102, 111, 111, 98], "decoding '666f6f62'");
  assertBytes(Uint8Array.fromHex("666f6f6261"), [102, 111, 111, 98, 97], "decoding '666f6f6261'");
  assertBytes(Uint8Array.fromHex("666f6f626172"), [102, 111, 111, 98, 97, 114], "decoding '666f6f626172'");
}
