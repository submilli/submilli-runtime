// expect-error: interface property `toString` must have type `() => string`
// expect-error-count: 1
// An interface property named `toString` is called by the same conversions
// as the method, so it must return a string; it may still be optional.
interface Counted {
  toString: () => number;
}

interface Labeled {
  toString?: () => string;
}

function main(): void {}
