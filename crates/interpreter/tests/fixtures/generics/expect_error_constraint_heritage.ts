// expect-error: does not satisfy the constraint
interface Parent<T extends string> { value: T; }
interface Child extends Parent<number> {}
function main(): void {}
