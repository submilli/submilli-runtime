// expect-error: expected `Required<T>`, got `T`
function fill<T>(value: T): Required<T> { return value; }
function main(): void {}
