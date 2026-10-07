// expect-error: `toJson` cannot be optional
// expect-error-count: 1
// `JSON.stringify(x)` calls `toJson` through the value's vtable, so an
// interface can't leave it out. An optional `toString` is allowed: an absent
// one falls back to `[object Object]`.
interface Labelled {
  toString?: () => string;
  toJson?: () => string;
  a: number;
}

function main(): void {}
