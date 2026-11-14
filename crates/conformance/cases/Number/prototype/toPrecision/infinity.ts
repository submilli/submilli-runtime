// test262: test/built-ins/Number/prototype/toPrecision/infinity.js
//
// The Number-object arms are dropped (no boxing).

function main(): void {
  assertSameValue(Infinity.toPrecision(1000), "Infinity", "Infinity value");
  assertSameValue((-Infinity).toPrecision(1000), "-Infinity", "-Infinity value");
}
