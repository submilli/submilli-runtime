// test262: test/built-ins/TypedArray/prototype/set/typedarray-arg-set-values-diff-buffer-same-type.js
// Instantiated at Uint8Array.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  const src: Uint8Array = new Uint8Array([42, 43]);

  let sample: Uint8Array = new Uint8Array([1, 2, 3, 4]);
  assertSameValue(sample.set(src, 1), undefined, "set returns undefined");
  assertBytes(sample, [1, 42, 43, 4], "offset: 1");

  sample = new Uint8Array([1, 2, 3, 4]);
  assertSameValue(sample.set(src, 0), undefined, "set returns undefined");
  assertBytes(sample, [42, 43, 3, 4], "offset: 0");

  sample = new Uint8Array([1, 2, 3, 4]);
  assertSameValue(sample.set(src, 2), undefined, "set returns undefined");
  assertBytes(sample, [1, 2, 42, 43], "offset: 2");
}
