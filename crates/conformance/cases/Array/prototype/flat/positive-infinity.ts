// test262: test/built-ins/Array/prototype/flat/positive-infinity.js
// expect-error: depth must be a non-negative integer literal
// By-design divergence (spec.md §1.2): flat's depth must be a non-negative
// integer literal so the result type is computable; Infinity is rejected at
// compile time instead of flattening fully.

function main(): void {
  const a = [[[1]], [[2], [3]]];
  const flattened = a.flat(Number.POSITIVE_INFINITY);
  assertSameValue(flattened.length, 3, "flattened.length");
}
