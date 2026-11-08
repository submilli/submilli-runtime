// expect-error: Type `string` cannot be used to index type `T`
function read<T>(value: T, key: string): unknown { return value[key]; }
function main(): void {}
