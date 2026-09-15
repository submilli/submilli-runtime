// test262: test/built-ins/TypedArray/prototype/subarray/results-with-different-length.js
// Instantiated at Uint8Array. Value-level behavior only — buffer sharing is
// rejected by design (subarray deep-copies; see
// cases/Uint8Array/divergence/subarray-deep-copy.ts).

function testRes(result: Uint8Array, expected: number[], msg: string): void {
  assertSameValue(result.length, expected.length, `${msg} (length)`);
  for (let i = 0; i < expected.length; i++) {
    assertSameValue(result[i], expected[i], `${msg} (index ${i})`);
  }
}

function main(): void {
  const sample: Uint8Array = new Uint8Array([40, 41, 42, 43]);

  testRes(sample.subarray(1), [41, 42, 43], "begin == 1");
  testRes(sample.subarray(2), [42, 43], "begin == 2");
  testRes(sample.subarray(3), [43], "begin == 3");

  testRes(sample.subarray(1, 4), [41, 42, 43], "begin == 1, end == length");
  testRes(sample.subarray(2, 4), [42, 43], "begin == 2, end == length");
  testRes(sample.subarray(3, 4), [43], "begin == 3, end == length");

  testRes(sample.subarray(0, 1), [40], "begin == 0, end == 1");
  testRes(sample.subarray(0, 2), [40, 41], "begin == 0, end == 2");
  testRes(sample.subarray(0, 3), [40, 41, 42], "begin == 0, end == 3");

  testRes(sample.subarray(-1), [43], "begin == -1");
  testRes(sample.subarray(-2), [42, 43], "begin == -2");
  testRes(sample.subarray(-3), [41, 42, 43], "begin == -3");

  testRes(sample.subarray(-1, 4), [43], "begin == -1, end == length");
  testRes(sample.subarray(-2, 4), [42, 43], "begin == -2, end == length");
  testRes(sample.subarray(-3, 4), [41, 42, 43], "begin == -3, end == length");

  testRes(sample.subarray(0, -1), [40, 41, 42], "begin == 0, end == -1");
  testRes(sample.subarray(0, -2), [40, 41], "begin == 0, end == -2");
  testRes(sample.subarray(0, -3), [40], "begin == 0, end == -3");

  testRes(sample.subarray(-0, -1), [40, 41, 42], "begin == -0, end == -1");
  testRes(sample.subarray(-0, -2), [40, 41], "begin == -0, end == -2");
  testRes(sample.subarray(-0, -3), [40], "begin == -0, end == -3");

  testRes(sample.subarray(-2, -1), [42], "length == 4, begin == -2, end == -1");
  testRes(sample.subarray(1, -1), [41, 42], "length == 4, begin == 1, end == -1");
  testRes(sample.subarray(1, -2), [41], "length == 4, begin == 1, end == -2");
  testRes(sample.subarray(2, -1), [42], "length == 4, begin == 2, end == -1");
}
