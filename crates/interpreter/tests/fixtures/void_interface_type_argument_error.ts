// expect-error: property `Cell.value`
interface Cell<T> { value: T; }
function take(x: Cell<void>): void {}
function main(): void {}
