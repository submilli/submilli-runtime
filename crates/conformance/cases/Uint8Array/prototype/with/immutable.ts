// test262: test/built-ins/TypedArray/prototype/with/immutable.js
// Instantiated at Uint8Array. The notSameValue identity checks are rewritten
// as a mutation probe (object identity doesn't port — README caveat).

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  const ta: Uint8Array = new Uint8Array([3, 1, 2]);
  ta.with(0, 2);
  assertBytes(ta, [3, 1, 2], "receiver is unchanged");

  const result: Uint8Array = ta.with(0, 2);
  result[1] = 99;
  assertBytes(ta, [3, 1, 2], "result is a new instance — writes don't reach the receiver");
}
