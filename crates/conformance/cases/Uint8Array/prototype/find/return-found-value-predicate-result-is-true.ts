// test262: test/built-ins/TypedArray/prototype/find/return-found-value-predicate-result-is-true.js
// Instantiated at Uint8Array.
// Adapted: the ToBoolean rows (predicates returning a string, object, Symbol or
// number) are dropped — the predicate must return boolean.

function main(): void {
  const sample = new Uint8Array([39, 2, 62]);
  let result: number | undefined;

  let called = 0;
  result = sample.find((): boolean => {
    called++;
    return true;
  });
  assertSameValue(result, 39, "returned true on sample[0]");
  assertSameValue(called, 1, "predicate was called once");

  called = 0;
  result = sample.find((val: number): boolean => {
    called++;
    return val === 62;
  });
  assertSameValue(called, 3, "predicate was called three times");
  assertSameValue(result, 62, "returned true on sample[3]");
}
