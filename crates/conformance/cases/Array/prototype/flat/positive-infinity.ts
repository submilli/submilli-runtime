// test262: test/built-ins/Array/prototype/flat/positive-infinity.js

function main(): void {
  const a = [[[1]], [[2], [3]]];
  const flattened = a.flat(Number.POSITIVE_INFINITY);
  assertSameValue(flattened.length, 3, "flattened.length");
}
