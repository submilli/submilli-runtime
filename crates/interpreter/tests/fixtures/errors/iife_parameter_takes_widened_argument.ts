// expect-error: expected `1`, got `number`
// An immediately-invoked function's parameter takes its argument's widened
// type, so it is a `number`, not the literal `1`.
function main(): void {
  ((x) => {
    const one: 1 = x;
  })(1);
}
