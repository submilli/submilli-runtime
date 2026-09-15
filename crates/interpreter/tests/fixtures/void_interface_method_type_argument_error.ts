// expect-error: method `Receiver.take`
interface Receiver<T> { take(x: T): void; }
function take(x: Receiver<void>): void {}
function main(): void {}
