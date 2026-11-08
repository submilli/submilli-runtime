// expect-error: generic type parameter defaults are not supported
function identity<T = number>(value: T): T { return value; }
function main(): void {}
