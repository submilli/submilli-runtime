// test262: test/built-ins/Array/prototype/includes/fromIndex-equal-or-greater-length-returns-false.js

function main(): void {
  const sample = [7, 7, 7, 7];
  assertSameValue(sample.includes(7, 4), false, "length");
  assertSameValue(sample.includes(7, 5), false, "length + 1");
}
