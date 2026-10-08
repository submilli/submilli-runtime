// expect-error: expected `)` after `for` update
// expect-error: expected `;` after `for` condition
// expect-error: expected expression
// expect-error-count: 3
// A destructured `for` loop whose header fails to parse reports only parse
// errors, not an internal compiler failure.
function main(): void {
  for (let [i, j] = [0, 6]; i < j; i++, j--) {}
  for (let [k] = [0]; k < 1 k++) {}
}
