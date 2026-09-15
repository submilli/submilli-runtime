// test262: test/built-ins/Array/prototype/includes/samevaluezero.js
// expect-fail: includes uses the equals vtable, which treats NaN !== NaN; the standard's SameValueZero says sample.includes(NaN) is true
// Adapted: the "42"/[42]/true/false/null/"" rows are compile-time type errors
// on a number[] receiver, so only the numeric rows port.

function main(): void {
  const sample: number[] = [42, 0, 1, NaN];
  assertSameValue(sample.includes(42.0), true, "42.0");
  assertSameValue(sample.includes(-0), true, "-0");
  assertSameValue(sample.includes(NaN), true, "NaN");
}
