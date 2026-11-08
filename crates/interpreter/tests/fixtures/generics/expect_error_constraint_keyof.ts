// expect-error: `keyof` of a type parameter is not supported
function key<T, K extends keyof T>(value: T, name: K): K { return name; }
function main(): void {}
