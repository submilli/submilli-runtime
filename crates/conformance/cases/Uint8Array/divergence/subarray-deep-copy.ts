// Divergence pin, derived from test262:
// test/built-ins/TypedArray/prototype/subarray/result-is-new-instance-with-shared-buffer.js
// (verbatim copy under rejected/Uint8Array/prototype/subarray/). In ECMA-262 a
// subarray is a view over the same ArrayBuffer — writes propagate both ways.
// Here Uint8Array owns its storage and subarray is a deep copy (spec.md §1.2):
// writes never propagate.

function assertBytes(actual: Uint8Array, expected: number[], message: string): void {
  assertSameValue(actual.length, expected.length, `${message} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(actual[i], expected[i], `${message} (index ${i})`);
  }
}

function main(): void {
  const sample: Uint8Array = new Uint8Array([40, 41, 42, 43]);
  const result: Uint8Array = sample.subarray(1);
  assertBytes(result, [41, 42, 43], "subarray values");

  sample[1] = 100;
  assertBytes(result, [41, 42, 43], "writes to the source do not reach the subarray");

  result[1] = 111;
  assertBytes(sample, [40, 100, 42, 43], "writes to the subarray do not reach the source");
}
