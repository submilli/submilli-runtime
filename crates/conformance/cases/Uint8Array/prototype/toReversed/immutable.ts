// test262: test/built-ins/TypedArray/prototype/toReversed/immutable.js
// Instantiated at Uint8Array. The notSameValue identity check is rewritten as
// a mutation probe (object identity doesn't port — README caveat).

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  const ta: Uint8Array = new Uint8Array([0, 1, 2]);
  ta.toReversed();
  assertBytes(ta, [0, 1, 2], "receiver is unchanged");

  const result: Uint8Array = ta.toReversed();
  result[0] = 99;
  assertBytes(ta, [0, 1, 2], "result is a new instance — writes don't reach the receiver");
}
