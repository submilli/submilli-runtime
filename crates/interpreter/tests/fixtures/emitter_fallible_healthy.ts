function verify(condition: boolean): void {
    if (!condition) { throw new Error("emitter regression"); }
}

function add(a: number, b: number = 2): number { return a + b; }
function sum(...values: number[]): number {
    let total = 0;
    for (const value of values) { total += value; }
    return total;
}

function empty(): { value: number } | null { return null; }

function main(): void {
    let order = "";
    const mark = (value: number): number => { order += value.toString(); return value; };
    const base = { first: mark(1), second: mark(2) };
    const copy = { ...base, second: mark(3) };
    verify(order === "123" && copy.first === 1 && copy.second === 3);
    const items = [mark(4), ...[mark(5), mark(6)]];
    verify(order === "123456" && items[2] === 6);
    verify(add(3) === 5 && sum(items[0], items[1], items[2]) === 15);
    const object: { value: number } | null = copy.first === 1 ? { value: 9 } : null;
    verify(object?.value === 9);
    const nothing = empty();
    verify((nothing?.value ?? 4) === 4);
    const large = 9007199254740993n;
    verify((large + 7n - 3n).toString() === "9007199254740997");
    let final = 0;
    for (let index = 0; index < 4; index++) {
        try {
            switch (index) {
                case 0: continue;
                case 1: final += 2; break;
                default: final += 3;
            }
        } finally { final += 1; }
    }
    verify(final === 12);
}
