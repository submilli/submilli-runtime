// An immediately invoked closure returns its value into the enclosing function.
// Its inferred union must fit that function's `void` return type, unlike a
// callback whose `void` context discards its result. tsc rejects both calls.
// expect-error: expected `void`, got `5 | "s"`
// expect-error: expected `void`, got `number | undefined`
// expect-error-count: 2
function g(x: boolean): void {
  return (() => {
    if (x) {
      return 5;
    }
  })();
}

function h(x: boolean): void {
  return (() => {
    if (x) {
      return 5;
    }
    return "s";
  })();
}

function main(): void {
  g(true);
  h(true);
}
