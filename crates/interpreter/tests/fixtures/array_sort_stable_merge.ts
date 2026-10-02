// `sort` and `toSorted` are stable merge sorts: equal elements keep their order
// across runs of every width, odd lengths included. The default order converts
// elements to strings only to compare them, as JavaScript does.
class Item {
    constructor(public key: number, public seq: number) {}
    toString(): string {
        return "k" + this.key.toString();
    }
}

let toStringCalls = 0;

class Counted {
    constructor(public key: string) {}
    toString(): string {
        toStringCalls += 1;
        return this.key;
    }
}

class Unprintable {
    toString(): string {
        throw new Error("no string form");
    }
}

class Wide {
    constructor(public label: string, public seq: number) {}
    toString(): string {
        return this.label;
    }
}

class Failing {
    constructor(public text: string, public fails: boolean) {}
    toString(): string {
        if (this.fails) {
            throw new Error("cannot print");
        }
        return this.text;
    }
}

function items(count: number): Item[] {
    const out: Item[] = [];
    for (let i = 0; i < count; i++) {
        out.push(new Item((i * 7) % 5, i));
    }
    return out;
}

function assertStable(sorted: Item[], label: string): void {
    for (let i = 1; i < sorted.length; i++) {
        const a = sorted[i - 1];
        const b = sorted[i];
        assert(a.key <= b.key, label + ": keys ascend at " + i.toString());
        if (a.key === b.key) {
            assert(a.seq < b.seq, label + ": equal keys keep their order at " + i.toString());
        }
    }
}

function main(): void {
    for (const count of [0, 1, 2, 3, 5, 8, 37, 64, 101]) {
        const byComparator = items(count);
        byComparator.sort((a: Item, b: Item) => a.key - b.key);
        assert(byComparator.length === count, "comparator sort keeps every element");
        assertStable(byComparator, "comparator " + count.toString());

        const byString = items(count);
        byString.sort();
        assertStable(byString, "default " + count.toString());

        const source = items(count);
        const copy = source.toSorted((a: Item, b: Item) => a.key - b.key);
        assertStable(copy, "toSorted " + count.toString());
        for (let i = 0; i < count; i++) {
            assert(source[i].seq === i, "toSorted leaves the receiver alone");
        }
    }

    // With fewer than two elements nothing is compared, so no toString() runs.
    [new Unprintable()].sort();
    [new Unprintable()].toSorted();
    const none: Unprintable[] = [];
    none.sort();
    const single = [new Counted("x")];
    single.sort();
    assert(toStringCalls === 0, "one element: no toString()");
    const pair = [new Counted("b"), new Counted("a")];
    pair.sort();
    assert(pair[0].key === "a" && pair[1].key === "b", "two elements sort by string");
    assert(toStringCalls === 2, "two elements: one comparison, two toString() calls");

    // A comparator answering 0 or NaN never reorders.
    const zeros = items(9);
    zeros.sort((a: Item, b: Item) => 0);
    for (let i = 0; i < zeros.length; i++) {
        assert(zeros[i].seq === i, "zero comparator keeps order");
    }
    const nans = items(9);
    nans.sort((a: Item, b: Item) => NaN);
    for (let i = 0; i < nans.length; i++) {
        assert(nans[i].seq === i, "NaN comparator keeps order");
    }

    // The default order compares UTF-16 code units and sorts null as "null".
    const mixed: (string | null)[] = ["b", null, "😀", "a", "￿", "nulm", "nulk"];
    mixed.sort();
    assert(mixed[0] === "a" && mixed[1] === "b", "a, b");
    assert(mixed[2] === "nulk" && mixed[3] === null && mixed[4] === "nulm", "null sorts as \"null\"");
    assert(mixed[5] === "😀" && mixed[6] === "￿", "surrogate pair before U+FFFF");

    // A comparator that throws propagates out of the sort.
    let thrown = "";
    try {
        items(10).sort((a: Item, b: Item) => {
            if (a.seq + b.seq > 5) {
                throw new Error("stop");
            }
            return a.key - b.key;
        });
    } catch (e) {
        thrown = e.message;
    }
    assert(thrown === "stop", "comparator error propagates");

    // Strings too large to keep for the whole sort are computed per comparison
    // instead: the order, its stability, and `null` handling are the same.
    const pad = "x".repeat(1000000);
    const wide: (Wide | null)[] = [];
    for (let i = 0; i < 6; i++) {
        wide.push(new Wide(((i * 7) % 3).toString() + pad, i));
        if (i % 2 === 0) {
            wide.push(null);
        }
    }
    wide.sort();
    for (let i = 0; i < 6; i++) {
        const w = wide[i];
        assert(w !== null, "wide strings sort before \"null\" at " + i.toString());
    }
    for (let i = 6; i < wide.length; i++) {
        assert(wide[i] === null, "nulls sort last at " + i.toString());
    }
    for (let i = 1; i < 6; i++) {
        const a = wide[i - 1];
        const b = wide[i];
        if (a === null || b === null) {
            assert(false, "wide strings sort before \"null\"");
            continue;
        }
        assert(a.label <= b.label, "wide keys ascend at " + i.toString());
        if (a.label === b.label) {
            assert(a.seq < b.seq, "wide equal keys keep their order at " + i.toString());
        }
    }

    // A toString() that throws stops the sort and leaves the array as it was,
    // whether strings are kept or computed per comparison. With 1.5M-unit
    // strings the first three pass the budget, so the last one first throws in
    // a comparison.
    for (const size of [10, 1500000]) {
        const failing: Failing[] = [];
        for (let i = 0; i < 4; i++) {
            failing.push(new Failing("v".repeat(size) + i.toString(), i === 3));
        }
        let failure = "";
        try {
            failing.sort();
        } catch (e) {
            failure = e.message;
        }
        assert(failure === "cannot print", "toString() error propagates");
        for (let i = 0; i < 4; i++) {
            assert(failing[i].text.endsWith(i.toString()), "failed sort leaves the array");
        }
    }

    // Larger inputs stay ordered.
    const values: number[] = [];
    let seed = 1;
    for (let i = 0; i < 500; i++) {
        seed = (seed * 1103515245 + 12345) % 2147483648;
        values.push(seed % 1000);
    }
    values.sort((a: number, b: number) => a - b);
    for (let i = 1; i < values.length; i++) {
        assert(values[i - 1] <= values[i], "500 numbers ascend");
    }

    const bytes = Uint8Array.alloc(37);
    for (let i = 0; i < bytes.length; i++) {
        bytes[i] = (i * 29) % 256;
    }
    bytes.sort((a: number, b: number) => b - a);
    for (let i = 1; i < bytes.length; i++) {
        assert(bytes[i - 1] >= bytes[i], "Uint8Array comparator sort descends");
    }
    const unsorted = Uint8Array.new([3, 1, 2, 255, 0]);
    unsorted.sort((a: number, b: number) => 0);
    unsorted.sort((a: number, b: number) => NaN);
    assert(
        unsorted[0] === 3 && unsorted[1] === 1 && unsorted[2] === 2 && unsorted[3] === 255,
        "Uint8Array zero/NaN comparator keeps order",
    );
}
