class Counter {
    calls: number = 0;
}
class Key {
    constructor(readonly value: string, private readonly counter: Counter, readonly seq: number = 0) {}
    toString(): string { this.counter.calls++; return this.value; }
}
function main(): void {
    const counter = new Counter();
    const prefix = "x".repeat(600000);
    const items: Key[] = [];
    for (let i = 0; i < 8; i++) {
        items.push(new Key(prefix + (7 - i).toString(), counter));
    }
    items.sort();
    assert(counter.calls === 8);
    assert(items[0].value === prefix + "0");
    assert(items[7].value === prefix + "7");
    const normal = ["", "a", "aa", "ab", "a", "😀", "z"];
    normal.sort();
    assert(normal.join("|") === "|a|a|aa|ab|z|😀");
    const first = new Key("same", counter, 1);
    const second = new Key("same", counter, 2);
    const stable = [first, second];
    stable.sort();
    assert(stable[0].seq === 1);
    assert(stable[1].seq === 2);
    const mixed: (string | null)[] = [null, "z", "a", null];
    mixed.sort();
    assert(mixed[0] === "a");
    assert(mixed[1] === null);
    assert(mixed[2] === null);
    assert(mixed[3] === "z");
}
