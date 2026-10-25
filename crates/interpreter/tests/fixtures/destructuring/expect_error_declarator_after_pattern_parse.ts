// expect-error: `const` declaration requires an initializer
// expect-error: expected `;` after declaration
// expect-error: `export` statements must appear at the top of the file
// expect-error-count: 3
// A destructuring declarator whose statement fails later (at another declarator,
// or as a nested `export`) reports only that error, not an internal compiler
// failure.
function main(): void {
  const [a] = [1], b;
  const [x] = [1], y = 2 z;
  export const [p] = [1], q = 2;
}
