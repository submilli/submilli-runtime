// expect-error: `toString` cannot be optional
// `String(x)` and `${x}` call `toString` through the value's vtable, so an
// interface can't leave it out, as an object type can't.
interface Labelled {
  toString?: () => string;
  a: number;
}

function main(): void {}
