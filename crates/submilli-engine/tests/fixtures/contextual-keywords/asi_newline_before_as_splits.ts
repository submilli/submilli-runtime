// A newline before `as` ends the statement (matching TypeScript): the second
// line is a call of the function named `as`, not a cast of `1` to type `(2)`.
function as(n: number): void {
  assert(n === 2);
}

function main(): void {
  const v = 1
  as(2);
  assert(v === 1);
}
