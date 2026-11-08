// expect-error: expected `T[K]`, got `string`
function write<T, K extends keyof T>(value: T, key: K): void { value[key] = "text"; }
function main(): void {}
