// expect-error: `never` cannot be used as a type argument
interface Sink<T> { emit(): T; }
function make<T>(): number { return 1; }
function main(): void { make<[Sink<void>, never]>(); }
