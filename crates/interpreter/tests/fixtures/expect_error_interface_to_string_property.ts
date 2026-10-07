// expect-error: interface property `toString` must have type `() => string` (got `() => number`)
// expect-error: interface property `toString` must have type `() => string` (got `() => T`)
// expect-error: interface property `toString` must have type `() => string` (got `null | (() => string)`)
// expect-error: interface property `toJson` must have type `() => string` (got `(...arg0: number[]) => string`)
// expect-error-count: 4
// An interface property named `toString` or `toJson` is called by the same
// conversions as the method, so it must have exactly the method's type; it
// may still be optional.
interface Counted {
  toString: () => number;
}

interface Generic<T> {
  toString: () => T;
}

interface Nullable {
  toString: (() => string) | null;
}

interface Variadic {
  toJson: (...xs: number[]) => string;
}

interface Labeled {
  toString?: () => string;
}

interface Fixed {
  toString: () => "fixed";
}

function main(): void {}
