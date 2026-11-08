// expect-error: expected `string`, got `keyof T`
function name<T>(key: keyof T): string { return key; }
function main(): void {}
