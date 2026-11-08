// expect-error: does not satisfy the constraint
function identity<T extends string>(value: T): T { return value; }
function main(): void { identity<number>(1); }
