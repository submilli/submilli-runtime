// test262: test/built-ins/Number/prototype/toExponential/infinity.js
//
// The Number-object arms are dropped (no boxing).

function main(): void {
  assertSameValue(Infinity.toExponential(1000), "Infinity", "Infinity value");
  assertSameValue((-Infinity).toExponential(1000), "-Infinity", "-Infinity value");
}
