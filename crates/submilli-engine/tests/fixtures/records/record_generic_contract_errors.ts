// expect-error: property `value` of type `number` does not satisfy string index value type `T`
// expect-error: incompatible inherited member
// expect-error: incompatible inherited string index signatures
interface Invalid<T> { [key: string]: T; value: number; }
type InvalidAlias<T> = { [key: string]: T; value: number };
interface Base<T> { [key: string]: T; value: T; }
interface Child<T> extends Base<T> { value: number; }
interface Other<U> { [key: string]: U; value: U; }
interface Different<T, U> extends Base<T>, Other<U> {}
interface Narrow<T> extends Base<T> { [key: string]: number; value: number; }
function main(): void {}
