// A function called on the spot returns its value into the call: a `void`
// context doesn't let its returns disagree or its body run off its end after
// returning a value, as a callback's `void` context does. tsc rejects both.
// expect-error: does not return a value on all paths
// expect-error: conflicts with earlier return
// expect-error: expected `void`, got `number`
// expect-error-count: 3
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
