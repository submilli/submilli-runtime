// test262: test/built-ins/TypedArray/prototype/find/return-found-value-predicate-result-is-true.js
// Instantiated at Uint8Array. The ToBoolean coercion rows (string / object /
// Symbol / number predicate returns) are dropped — predicates are typed
// boolean.

function main(): void {
  const sample: Uint8Array = new Uint8Array([39, 2, 62]);

  let called: number = 0;
  let result: number | undefined = sample.find((v: number): boolean => {
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
  assertSameValue(result, 62, "returned true on sample[2]");

  result = sample.find((v: number): boolean => false);
  assertSameValue(result, undefined, "no match returns undefined");
}
