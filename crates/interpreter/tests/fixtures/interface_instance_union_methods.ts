// SUB-2274: a method called on a union of one interface's instances
// dispatches as on one instance: each member is a value of the interface. The
// call returns the union of the members' results.
interface Box<T> {
    name(): string;
    label(prefix?: string): string;
    count(...items: number[]): number;
    get(): T;
    held: T;
}
class NumberBox implements Box<number> {
    held: number = 1;
    name(): string { return "number"; }
    label(prefix?: string): string { return (prefix ?? "") + "n"; }
    count(...items: number[]): number { return items.length; }
    get(): number { return this.held; }
}
class StringBox implements Box<string> {
    held: string = "s";
    name(): string { return "string"; }
    label(prefix?: string): string { return (prefix ?? "") + "s"; }
    count(...items: number[]): number { return items.length * 2; }
    get(): string { return this.held; }
}
function describe(b: Box<number> | Box<string>): string {
    return b.name() + ":" + String(b.get()) + ":" + String(b.held);
}
function labels(b: Box<number> | Box<string> | Box<boolean>): string {
    return b.label() + b.label(">") + String(b.count(1, 2));
}
function first(x: Iterator<number> | Iterator<string>): string {
    const r = x.next();
    if (r.done) {
        return "done";
    }
    return typeof r.value;
}
type Either = Iterator<number> | Iterator<string>;
function firstOf(x: Either): string {
    return first(x);
}
function maybeFirst(x: Iterator<number> | Iterator<string> | undefined): string {
    const r = x?.next();
    return r === undefined ? "none" : String(r.value);
}
function main(): void {
    const literal: Box<number> = {
        held: 7,
        name: () => "literal",
        label: (prefix?: string) => (prefix ?? "") + "l",
        count: (...items: number[]) => items.length,
        get: () => 7,
    };
    assert(describe(new NumberBox()) === "number:1:1", "class instance of one member");
    assert(describe(new StringBox()) === "string:s:s", "class instance of the other");
    assert(describe(literal) === "literal:7:7", "object literal");
    assert(labels(new NumberBox()) === "n>n2", "optional and rest parameters");
    assert(labels(new StringBox()) === "s>s4", "the other member's method");
    assert(labels(literal) === "l>l2", "a union of three instances");
    assert(first("a,b".split(",").values()) === "string", "array iterator");
    assert(first(new Set<number>([1]).values()) === "number", "set iterator");
    assert(first(new Map<string, number>().keys()) === "done", "empty map iterator");
    assert(firstOf([2].values()) === "number", "through an alias of the union");
    assert(maybeFirst(undefined) === "none", "optional call on undefined");
    assert(maybeFirst(["x"].values()) === "x", "optional call on an iterator");
}
