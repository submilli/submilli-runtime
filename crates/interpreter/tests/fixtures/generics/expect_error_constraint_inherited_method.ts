// expect-error: expected `string`, got `42`
interface Parent<T> { pick<U extends T>(value: U): U; }
interface Child extends Parent<string> {}
function bad(value: Child): void { value.pick(42); }
function main(): void {}
