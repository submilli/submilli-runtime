// expect-error: expected `number`, got `T[keyof T]`
function read<T extends { a: number }>(value: T, key: keyof T): number { return value[key]; }
function main(): void {}
