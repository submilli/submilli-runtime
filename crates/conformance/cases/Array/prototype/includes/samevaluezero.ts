// test262: test/built-ins/Array/prototype/includes/samevaluezero.js
// Adapted: the "42"/[42]/true/false/null/"" rows are compile-time type errors
// on a number[] receiver, so only the numeric rows port.

function main(): void {
  const sample: number[] = [42, 0, 1, NaN];
  assertSameValue(sample.includes(42.0), true, "42.0");
  assertSameValue(sample.includes(-0), true, "-0");
  assertSameValue(sample.includes(NaN), true, "NaN");
}
