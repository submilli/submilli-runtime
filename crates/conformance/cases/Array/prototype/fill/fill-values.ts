// test262: test/built-ins/Array/prototype/fill/fill-values.js
// Adapted: the `[]` receiver is a `number[]` binding (an untyped `[]` has
// element type never, so fill(8) would not type-check). The `[0, 0].fill()`
// row is dropped — `value` is a required parameter in the typed signature, as
// in TypeScript.
// The start-argument rows belong to fill-values-relative-start.js (not ported)
// and the end-argument rows to fill-values-relative-end.js (ported separately).

function main(): void {
  const empty: number[] = [];
  assertCompareArray(empty.fill(8), [], "[].fill(8) must return []");

  assertCompareArray([0, 0, 0].fill(8), [8, 8, 8],
    "[0, 0, 0].fill(8) must return [8, 8, 8]",
  );
}
