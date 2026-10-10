function grow(xs: number[]): number {
    xs.push(99);
    return 7;
}

function main(): string {
    const xs: number[] = [];
    let sum = 0;
    for (let i = 0; i < 1000; i++) sum += xs.push(i);
    const alias = xs;
    alias.pop();
    alias.push(123);
    xs[0] = grow(xs);
    const snapshot = [...xs, grow(xs)];
    const live: number[] = [];
    live.push(1);
    let visits = 0;
    for (const n of live) {
        visits++;
        if (n < 30) live.push(n + 1);
    }
    const mapped = live.map((n: number) => {
        live.push(100);
        return n * 2;
    });
    const result = [sum, xs.length, xs[0], xs[999], snapshot.length,
                    visits, live.length, mapped.length, mapped[29]].join(",");
    assert(result === "500500,1002,7,123,1002,30,60,30,60");
    return result;
}
