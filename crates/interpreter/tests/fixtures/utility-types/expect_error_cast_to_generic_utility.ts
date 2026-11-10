// expect-error: generic type parameters are erased at runtime
function fill<T>(value: Partial<T>): T { return value as T; }
function main(): void {}
