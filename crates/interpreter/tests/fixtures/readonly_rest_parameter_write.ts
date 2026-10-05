// A `readonly` rest parameter only permits reading.
// expect-error: cannot assign to an element of `readonly string[]`
// expect-error: cannot call `push` on `readonly string[]`
// expect-error-count: 2
function f(...args: readonly string[]): void {
  args[0] = "abc";
  args.push("def");
}

function main(): void {}
