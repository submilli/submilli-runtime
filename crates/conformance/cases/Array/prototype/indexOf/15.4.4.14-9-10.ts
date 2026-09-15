// test262: test/built-ins/Array/prototype/indexOf/15.4.4.14-9-10.js
// Adapted: numeric subset of the heterogeneous sample — the point (strict
// equality means indexOf never finds NaN) is type-expressible.

function main(): void {
  const a: number[] = [0, NaN, 1, NaN];

  assertSameValue(a.indexOf(NaN), -1, "NaN is equal to nothing, including itself.");
}
