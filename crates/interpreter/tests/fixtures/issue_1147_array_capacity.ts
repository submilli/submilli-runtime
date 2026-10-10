function grow(xs: number[]): number {
    xs.push(99);
    return 7;
}

function shrink(xs: number[]): number {
    xs.pop();
    return 8;
}

function main(): void {
    const xs: number[] = [];
    const alias = xs;
    for (let i = 0; i < 300; i++) {
        assert(xs.push(i) === i + 1);
    }
    assert(alias.length === 300 && alias[299] === 299);
    for (let i = 0; i < 300; i++) assert(xs[i] === i);

    const small: number[] = [];
    small.push(1);
    small.push(2);
    small.push(3);
    assert(small.length === 3);
    assert(JSON.stringify(small) === "[1,2,3]");
    assert(small.join(",") === "1,2,3");
    assert([...small].length === 3);
    const [first, ...rest] = small;
    assert(first === 1 && rest.length === 2);
    const unknownArray: unknown = small;
    const checked = unknownArray as number[];
    assert(checked.length === 3);
    const tuple = unknownArray as [number, number, number];
    assert(tuple[2] === 3);
    assert(Array.from(small).length === 3);
    assert(new Set(small).size === 3);
    assert(small.map((n: number) => n * 2).join(",") === "2,4,6");
    let sum = 0;
    for (const n of small) sum += n;
    assert(sum === 6);

    let readThrew = false;
    try { const value = small[3]; } catch (e) { readThrew = true; }
    assert(readThrew);
    let appendThrew = false;
    try { small[small.length] = 4; } catch (e) { appendThrew = true; }
    assert(appendThrew && small.length === 3);
    let gapThrew = false;
    try { small[5] = 4; } catch (e) { gapThrew = true; }
    assert(gapThrew && small.length === 3);

    // The RHS can grow within existing capacity, or shrink without releasing it.
    small[3] = grow(small);
    assert(small[3] === 7 && small.length === 4);
    let shrinkThrew = false;
    try { small[3] = shrink(small); } catch (e) { shrinkThrew = true; }
    assert(shrinkThrew && small.length === 3);
    small[0]++;
    assert(small[0] === 2);
    assert(small.pop() === 3);
    small.push(4);
    assert(small.join(",") === "2,2,4");
    small.splice(1, 2);
    small.push(5);
    assert(small.join(",") === "2,5");
    small.shift();
    small.unshift(6);
    assert(small.join(",") === "6,5");
    small.reverse();
    assert(small.join(",") === "5,6");

    const live: number[] = [];
    live.push(1);
    let visits = 0;
    for (const n of live) {
        visits++;
        if (n < 30) live.push(n + 1);
    }
    assert(visits === 30 && live.length === 30);

    const refs: ({value: number} | null)[] = [];
    for (let i = 0; i < 100; i++) refs.push({value: i});
    refs.push(null);
    assert(refs.pop() === null);
    const last = refs.pop();
    assert(last !== null && last !== undefined && last.value === 99);
    refs.push({value: 123});
    assert(refs.length === 100);
    const copy = refs.map((value: {value: number} | null) => {
        refs.push(null);
        return value;
    });
    assert(copy.length === 100 && refs.length === 200);
    assert(JSON.stringify(copy).includes("123"));
}
