interface Counter { n: number; }
class Stored implements Counter { n: number = 1; }
class Accessor implements Counter {
    private held: number = 1;
    reads: number = 0;
    writes: number = 0;
    get n(): number { this.reads++; return this.held; }
    set n(v: number) { this.writes++; this.held = v; }
}
function bump(s: Counter): number { return s.n++; }
function lower(s: Counter): number { return s.n--; }
function main(): void {
    const data: Counter = new Stored();
    assert(bump(data) === 1, "data old increment value");
    assert(lower(data) === 2, "data old decrement value");
    const acc = new Accessor();
    let calls = 0;
    const receiver = (): Counter => { calls++; return acc; };
    assert(receiver().n++ === 1, "accessor old increment value");
    assert(calls === 1, "receiver evaluated once");
    assert(acc.reads === 1 && acc.writes === 1, "one getter and setter call");
    assert(lower(acc) === 2, "accessor old decrement value");
    assert(acc.reads === 2 && acc.writes === 2, "one call for decrement");
}
