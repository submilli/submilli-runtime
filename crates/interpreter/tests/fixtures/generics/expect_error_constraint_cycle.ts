// expect-error: circular constraint
function cycle<T extends U, U extends T>(value: T): T { return value; }
function main(): void {}
