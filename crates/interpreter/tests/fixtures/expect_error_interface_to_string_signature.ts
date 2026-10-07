// expect-error: interface method `toString` must have signature `(): string`
// expect-error-count: 1
// `toString` fills the conversion slot that `String(x)` and interpolation
// call, which returns a string, so an interface may only declare it so, as a
// class may.
interface Numbered {
  toString(): number;
}

function describe(n: Numbered): number {
  return n.toString();
}

function main(): void {}
