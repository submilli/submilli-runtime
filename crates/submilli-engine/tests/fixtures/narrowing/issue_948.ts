class K { v: number = 1; }
function read<T>(x: T): T {
    if (x instanceof K) {
        assert(x.v === 1);
        return x;
    }
    if (typeof x === "string") {
        assert(x.toUpperCase() === "HI");
        return x;
    }
    if (Array.isArray(x)) {
        assert(x.length === 2);
        return x;
    }
    return x;
}
export function main(): void {
    assert(read(new K()).v === 1);
    assert(read("hi") === "hi");
    assert(read([1, 2]).length === 2);
    assert(read(3) === 3);
    assert(aliased("hi") === "hi");
    assert(union("hi") === "hi");
    assert(union(new K()) instanceof K);
}

type Identity<T> = T;
function aliased<T>(x: Identity<T>): T {
    if (typeof x === "string") { const copy = x; assert(copy.length === 2); return copy; }
    return x;
}
function union<T>(x: T | number): T | number {
    if (typeof x === "string") { assert(x.length === 2); return x; }
    if (x instanceof K) { assert(x.v === 1); return x; }
    return x;
}
