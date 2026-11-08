// expect-error: circular constraint
function invalid<T extends T | string>(value: T): T { return value; }
function main(): void {}
