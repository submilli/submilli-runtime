// An interface whose `toString` is an optional property keeps that property's
// rules: the call needs a guard, as in tsc, rather than falling back to the
// default every value has.
// expect-error-count: 1
// expect-error: cannot call value of type
interface Labelled {
  toString?: () => string;
  x: number;
}

function f(value: Labelled): string {
  return value.toString();
}

function main(): void {}
