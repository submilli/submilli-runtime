// expect-error: destructuring pattern has 3 elements, but right-hand side tuple has 1 element
// expect-error: destructuring pattern has 3 elements, but right-hand side tuple has 1 element
// expect-error-count: 2
// A slot past the literal's end without a default has nothing to read, as tsc
// reports (TS2493). The pattern is reported once, however many slots are missing.
function main(): void {
  const [a, b = 2, c] = [1];
  const [d, , e] = [1];
}
