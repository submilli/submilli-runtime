// A function called on the spot returns its value into the call: a `void`
// context doesn't let its returns disagree, as a callback's `void` context
// does, and one that can run off its end after returning a value returns
// `number | undefined`, which isn't `void`. tsc rejects both.
// expect-error: conflicts with earlier return
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
