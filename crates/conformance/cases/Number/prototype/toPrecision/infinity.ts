// test262: test/built-ins/Number/prototype/toPrecision/infinity.js
// expect-fail: toPrecision checks the 1..100 precision range before the Infinity short-circuit; the standard returns "Infinity"/"-Infinity" for an infinite receiver regardless of precision
//
// The Number-object arms are dropped (no boxing).

function main(): void {
  assertSameValue(Infinity.toPrecision(1000), "Infinity", "Infinity value");
  assertSameValue((-Infinity).toPrecision(1000), "-Infinity", "-Infinity value");
}
