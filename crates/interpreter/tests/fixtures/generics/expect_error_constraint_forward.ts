// expect-error: does not satisfy the constraint
interface Earlier { box: Box<string>; }
interface Box<T extends number> { value: T; }
function main(): void {}
