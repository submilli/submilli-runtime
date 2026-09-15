// test262: test/built-ins/TypedArray/prototype/subarray/results-with-same-length.js
// Instantiated at Uint8Array. Value-level behavior only — buffer sharing is
// rejected by design (subarray deep-copies; see
// cases/Uint8Array/divergence/subarray-deep-copy.ts).

function testRes(result: Uint8Array, msg: string): void {
  assertSameValue(result.length, 4, msg);
  assertSameValue(result[0], 40, `${msg} & result[0] === 40`);
  assertSameValue(result[1], 41, `${msg} & result[1] === 41`);
  assertSameValue(result[2], 42, `${msg} & result[2] === 42`);
  assertSameValue(result[3], 43, `${msg} & result[3] === 43`);
}

function main(): void {
  const sample: Uint8Array = new Uint8Array([40, 41, 42, 43]);

  testRes(sample.subarray(0), "begin == 0");
  testRes(sample.subarray(-4), "begin == -srcLength");
  testRes(sample.subarray(-5), "begin < -srcLength");

  testRes(sample.subarray(0, 4), "begin == 0, end == srcLength");
  testRes(sample.subarray(-4, 4), "begin == -srcLength, end == srcLength");
  testRes(sample.subarray(-5, 4), "begin < -srcLength, end == srcLength");

  testRes(sample.subarray(0, 5), "begin == 0, end > srcLength");
  testRes(sample.subarray(-4, 5), "begin == -srcLength, end > srcLength");
  testRes(sample.subarray(-5, 5), "begin < -srcLength, end > srcLength");
}
