// An array literal of regular literal types has those literal types as its
// element type, as in TypeScript, so a different literal can't be added.
// expect-error: expected `"hello"`, got `"other"`
// expect-error: expected `1 | 2`, got `3`
// expect-error-count: 2
function build(h: "hello", n: 1 | 2): void {
  const greetings = [h];
  greetings.push("other");
  let sizes = [n, n];
  sizes[0] = 3;
}

function main(): void {}
