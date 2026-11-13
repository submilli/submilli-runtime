// SUB-2274: a method of a union of one interface's instances is offered only
// when every member takes the same parameters, as tsc rejects an argument no
// member's parameter accepts together.
// expect-error: `put` is a method on `Box<number>`, and a union receiver reads only fields
interface Box<T> {
    put(value: T): void;
}
function store(b: Box<number> | Box<string>): void {
    b.put(1);
}
function main(): void {}
