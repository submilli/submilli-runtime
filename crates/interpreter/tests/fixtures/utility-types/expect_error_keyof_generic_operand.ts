// A wider operand has fewer keys: a key of the bound need not be a key of
// `T[K]` or `Pick<T, K>` (TS2322).
// expect-error: expected `keyof T[K]`, got `"toFixed"`
// expect-error: expected `keyof Pick<T, K>`, got `"a"`
// expect-error-count: 2
function property<T extends { a: number }, K extends keyof T>(value: T, key: K): keyof T[K] { return "toFixed"; }
function picked<T extends { a: number; b: number }, K extends keyof T>(value: T, key: K): keyof Pick<T, K> { return "a"; }
function main(): void {}
