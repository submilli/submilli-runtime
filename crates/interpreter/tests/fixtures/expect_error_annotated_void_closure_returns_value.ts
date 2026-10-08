// A closure annotated as returning `void` still can't return a value, as in
// tsc; only a `void` context lets the value go.
// expect-error: expected `void`, got `number`
// expect-error-count: 1
function main(): void {
  const f = (): void => {
    if (Math.random() > 2) {
      return;
    }
    return 5;
  };
  f();
}
