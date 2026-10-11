// test262: test/built-ins/TypedArray/prototype/set/typedarray-arg-set-values-diff-buffer-same-type.js
// Instantiated at Uint8Array.
// Adapted: compareArray is a local helper over Uint8Array (the harness's
// compareArray takes arrays), and `"..." + sample` is spelled with
// `sample.join(",")` (Uint8Array has no string conversion here; join(",") is
// what toString returns).

function compareArray(actual: Uint8Array, expected: number[]): boolean {
  if (actual.length !== expected.length) {
    return false;
  }
  for (let i = 0; i < expected.length; i++) {
    if (!Object.is(actual[i], expected[i])) {
      return false;
    }
  }
  return true;
}

function main(): void {
  const src = new Uint8Array([42, 43]);

  let sample = new Uint8Array([1, 2, 3, 4]);
  let result = sample.set(src, 1);
  assert(compareArray(sample, [1, 42, 43, 4]), "offset: 1, result: " + sample.join(","));
  assertSameValue(result, undefined, "returns undefined");

  sample = new Uint8Array([1, 2, 3, 4]);
  result = sample.set(src, 0);
  assert(compareArray(sample, [42, 43, 3, 4]), "offset: 0, result: " + sample.join(","));
  assertSameValue(result, undefined, "returns undefined");

  sample = new Uint8Array([1, 2, 3, 4]);
  result = sample.set(src, 2);
  assert(compareArray(sample, [1, 2, 42, 43]), "offset: 2, result: " + sample.join(","));
  assertSameValue(result, undefined, "returns undefined");
}
