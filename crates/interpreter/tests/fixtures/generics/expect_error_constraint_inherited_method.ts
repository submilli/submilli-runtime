// expect-error: does not satisfy the constraint
interface Parent<T> { pick<U extends T>(value: U): U; }
interface Child extends Parent<string> {}
function bad(value: Child): void { value.pick(42); }
function main(): void {}
