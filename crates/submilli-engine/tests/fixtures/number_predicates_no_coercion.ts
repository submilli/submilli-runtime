class Counter { calls: number = 0; }
class Numeric {
    constructor(private readonly counter: Counter) {}
    valueOf(): number { this.counter.calls++; return 1; }
}
function main(): void {
    const counter = new Counter();
    const values: unknown[] = [null, "x".repeat(256), 2n ** 1024n, true, {}, new Numeric(counter)];
    for (const value of values) {
        assert(!Number.isNaN(value));
        assert(!Number.isFinite(value));
        assert(!Number.isInteger(value));
        assert(!Number.isSafeInteger(value));
    }
    assert(counter.calls === 0);
    assert(Number.isNaN(NaN));
    assert(Number.isFinite(1));
    assert(Number.isInteger(2));
    assert(Number.isSafeInteger(3));
}
