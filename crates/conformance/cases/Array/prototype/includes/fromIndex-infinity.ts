// test262: test/built-ins/Array/prototype/includes/fromIndex-infinity.js

function main(): void {
  const sample = [7, 7, 7, 7];
  assertSameValue(sample.includes(7, Infinity), false, "fromIndex: Infinity");
  assertSameValue(sample.includes(7, -Infinity), true, "fromIndex: -Infinity");
}
