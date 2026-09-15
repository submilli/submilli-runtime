// test262: test/built-ins/TypedArray/prototype/findLast/return-found-value-predicate-result-is-true.js
// Instantiated at Uint8Array. The ToBoolean coercion rows are dropped —
// predicates are typed boolean. JS's undefined miss maps to null.

function main(): void {
  const sample: Uint8Array = new Uint8Array([39, 2, 62]);

  let called: number = 0;
  let result: number | null = sample.findLast((v: number): boolean => {
    called++;
    return true;
  });
  assertSameValue(result, 62, "returned true on sample[2] (last)");
  assertSameValue(called, 1, "predicate was called once");

  called = 0;
  result = sample.findLast((val: number): boolean => {
    called++;
    return val === 39;
  });
  assertSameValue(called, 3, "predicate was called three times");
  assertSameValue(result, 39, "returned true on sample[0]");

  result = sample.findLast((v: number): boolean => false);
  assertSameValue(result, null, "no match returns null");
}
