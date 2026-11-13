// SUB-2275: a built-in iterator's `toString` names its JavaScript tag
// (`Symbol.toStringTag`), as `Object.prototype.toString` does.
function show(x: Iterator<string>): string {
    return x.toString();
}
function showEither(x: Iterator<string> | Iterator<number>): string {
    return x.toString();
}
class Counter implements Iterator<number> {
    private n: number = 0;
    next(): IteratorResult<number> {
        this.n += 1;
        return this.n > 1 ? { done: true, value: undefined } : { done: false, value: this.n };
    }
}
class Named implements Iterator<number> {
    next(): IteratorResult<number> {
        return { done: true, value: undefined };
    }
    toString(): string {
        return "named";
    }
}
function showNumbers(x: Iterator<number>): string {
    return x.toString();
}
function main(): void {
    assert(show("a,b".split(",").values()) === "[object Array Iterator]", "array values");
    assert(showEither([1].keys()) === "[object Array Iterator]", "array keys");
    assert(String([1].entries()) === "[object Array Iterator]", "array entries");
    assert(String(new Uint8Array(2).values()) === "[object Array Iterator]", "bytes");
    const map = new Map<string, number>([["a", 1]]);
    assert(String(map.keys()) === "[object Map Iterator]", "map keys");
    assert(`${map.entries()}` === "[object Map Iterator]", "map entries");
    assert(String(map.values()) === "[object Map Iterator]", "map values");
    const set = new Set<number>([1]);
    assert(String(set.values()) === "[object Set Iterator]", "set values");
    assert(String(set.keys()) === "[object Set Iterator]", "set keys");
    assert(String(set.entries()) === "[object Set Iterator]", "set entries");
    assert(String(new Uint8Array(2).keys()) === "[object Array Iterator]", "bytes keys");
    assert(String(new Uint8Array(2).entries()) === "[object Array Iterator]", "bytes entries");
    assert(showNumbers(new Counter()) === "[object Object]", "a class's iterator");
    assert(showNumbers(new Named()) === "named", "an iterator's own toString");
    assert(String("abab".matchAll(/a/g)) === "[object RegExp String Iterator]", "matchAll");
    const it = [1, 2].values();
    it.next();
    assert(String(it) === "[object Array Iterator]", "after a step");
    assert(JSON.stringify(it) === "{}", "JSON of an iterator");
}
