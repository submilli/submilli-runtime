// As in TypeScript, a function declaration's binding cannot be reassigned.
// expect-error: cannot assign to function `f`
// expect-error-count: 1
function main(): void {
  function f(): void {}
  f = () => {};
}
