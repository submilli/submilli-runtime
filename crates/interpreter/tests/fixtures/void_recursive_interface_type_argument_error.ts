// expect-error: type argument containing `void`
interface Loop<T> { next: Loop<T[]>; get(): T; }
function inspect(x: Loop<void>): void {}
function main(): void {}
