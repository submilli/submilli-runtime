// A key bounded only by `string` may name no property of `T` (TS2536).
// expect-error: Type `K` cannot be used to index type `T`
// expect-error-count: 2
function read<T, K extends string>(value: T, key: K): T[K] { return value[key]; }
function main(): void {}
