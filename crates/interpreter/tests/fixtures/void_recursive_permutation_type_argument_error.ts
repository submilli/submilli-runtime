// expect-error: method `Swap.write`
interface Swap<A, B> { read(): A; write(x: B): void; next: Swap<B, A>; }
function inspect(x: Swap<void, number>): void {}
function main(): void {}
