// tsc's TS2536, at the declaration: `"a"` is no key of `T`'s bound.
// expect-error: Type `"a"` cannot be used to index type `T`
// expect-error: expected `T["a"]`, got `number`
// expect-error-count: 2
function read<T extends { b: number }>(value: T): T["a"] { return value.b; }
function main(): void {}
