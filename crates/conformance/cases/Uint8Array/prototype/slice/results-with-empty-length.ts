// test262: test/built-ins/TypedArray/prototype/slice/results-with-empty-length.js
// Instantiated at Uint8Array; the hasOwnProperty(0) checks are dropped (no
// property machinery — length === 0 carries the intent).

function testRes(result: Uint8Array, msg: string): void {
  assertSameValue(result.length, 0, msg);
}

function main(): void {
  const sample: Uint8Array = new Uint8Array([40, 41, 42, 43]);

  testRes(sample.slice(4), "begin == length");
  testRes(sample.slice(5), "begin > length");

  testRes(sample.slice(4, 4), "begin == length, end == length");
  testRes(sample.slice(5, 4), "begin > length, end == length");

  testRes(sample.slice(4, 5), "begin == length, end > length");
  testRes(sample.slice(5, 5), "begin > length, end > length");

  testRes(sample.slice(0, 0), "begin == 0, end == 0");
  testRes(sample.slice(-0, -0), "begin == -0, end == -0");
  testRes(sample.slice(1, 0), "begin > 0, end == 0");
  testRes(sample.slice(-1, 0), "being < 0, end == 0");

  testRes(sample.slice(2, 1), "begin > 0, begin < length, begin > end, end > 0");
  testRes(sample.slice(2, 2), "begin > 0, begin < length, begin == end");

  testRes(sample.slice(2, -2), "begin > 0, begin < length, end == -2");

  testRes(sample.slice(-1, -1), "length = 4, begin == -1, end == -1");
  testRes(sample.slice(-1, -2), "length = 4, begin == -1, end == -2");
  testRes(sample.slice(-2, -2), "length = 4, begin == -2, end == -2");

  testRes(sample.slice(0, -4), "begin == 0, end == -length");
  testRes(sample.slice(-4, -4), "begin == -length, end == -length");
  testRes(sample.slice(-5, -4), "begin < -length, end == -length");

  testRes(sample.slice(0, -5), "begin == 0, end < -length");
  testRes(sample.slice(-4, -5), "begin == -length, end < -length");
  testRes(sample.slice(-5, -5), "begin < -length, end < -length");
}
