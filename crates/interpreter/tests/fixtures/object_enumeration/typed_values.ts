// `Object.values` and `Object.entries` keep the value type, as tsc's
// `values<T>(o: { [s: string]: T } | ArrayLike<T>): T[]` overload infers it.

interface Seen {
    created: string;
    n: number;
}

interface SeenById {
    [id: string]: Seen;
}

type Point = { x: number; label: string };

enum Status {
    Open = "open",
    Done = "done",
}

type XY = { x: number; y: number };

function sumXY(point: XY): number {
    return sum(Object.values(point));
}

function sum(xs: number[]): number {
    return xs.reduce((total, x) => total + x, 0);
}

function totalOf(byName: Record<string, number> | undefined): number {
    return sum(Object.values(byName ?? {}));
}

function main(): void {
    // The loop from SUB-2104.
    const seen: Record<string, Seen> = {};
    seen["a"] = { created: "2023-05-01", n: 1 };
    seen["b"] = { created: "2024-01-01", n: 2 };
    let may = 0;
    for (const [key, v] of Object.entries(seen)) {
        if (v.created.startsWith("2023-05")) may += v.n;
        assert(key === "a" || key === "b");
    }
    assert(may === 1);
    assert(sum(Object.values(seen).map((v) => v.n)) === 3);

    const indexed: { [k: string]: Seen } = { c: { created: "2025", n: 4 } };
    assert(Object.values(indexed)[0].created === "2025");
    const byId: SeenById = { d: { created: "2026", n: 5 } };
    const [id, found] = Object.entries(byId)[0];
    assert(id === "d" && found.n === 5);

    const byNumber: Record<number, string> = {};
    byNumber[1] = "one";
    byNumber[2] = "two";
    assert(Object.entries(byNumber).map(([k, v]) => k + v.toUpperCase()).join() === "1ONE,2TWO");
    const byKey: Record<"x" | "y", number> = { x: 1, y: 2 };
    assert(sum(Object.values(byKey)) === 3);

    // An object type has tsc's implicit index signature: the union of its fields.
    const point: Point = { x: 3, label: "p" };
    const fields: (number | string)[] = Object.values(point);
    assert(fields.length === 2);
    assert(sum(Object.values({ a: 1, b: 2 })) === 3);
    // A present optional field can hold `undefined`, so it adds `undefined`,
    // as its read does (tsc leaves it out).
    const partial: { a?: number; b?: number } = { a: 2, b: undefined };
    const counts: (number | undefined)[] = Object.values(partial);
    assert(counts.length === 2);
    assert(sum(Object.values(partial).filter((n): n is number => n !== undefined)) === 2);

    // Arrays, tuples and strings list their elements.
    const xs: readonly number[] = [3, 4];
    assert(sum(Object.values(xs)) === 7);
    assert(Object.entries(xs).map(([i, x]) => i + ":" + x.toFixed(1)).join() === "0:3.0,1:4.0");
    const pair: [number, string] = [1, "s"];
    const parts: (number | string)[] = Object.values(pair);
    assert(parts.length === 2);
    assert(Object.values("hé").join("|") === "h|é");

    // Nullish members list nothing; `{}` adds no type.
    assert(totalOf({ a: 1, b: 2 }) === 3);
    assert(totalOf(undefined) === 0);

    const statuses: Record<string, Status> = { a: Status.Open, b: Status.Done };
    assert(Object.values(statuses).filter((s) => s === Status.Done).length === 1);
    const handlers: Record<string, () => number> = { one: () => 1, two: () => 2 };
    assert(sum(Object.values(handlers).map((f) => f())) === 3);
    const nested: Record<string, Record<string, number>> = { a: { x: 1 }, b: { y: 2, z: 3 } };
    assert(sum(Object.values(nested).map((r) => sum(Object.values(r)))) === 6);
    // Literals of one type unite, and a candidate the others fit wins, as tsc
    // infers them.
    const ones: Record<string, 1> | Record<string, 2> = byKey.x === 1 ? { a: 1 } : { b: 2 };
    const oneOrTwo: (1 | 2)[] = Object.values(ones);
    assert(oneOrTwo.length === 1);
    assert(sum(widest({ a: 1 })) === 1);
    assert(Object.values(statusOf({ a: Status.Done })).length === 1);
    assert(firstCreated([{ created: "2027", n: 6 }]) === "2027");
    assert(firstCreated({ e: { created: "2028", n: 7 } }) === "2028");

    const indexedEntries = Object.entries(indexed);
    assert(indexedEntries[0][0] === "c" && indexedEntries[0][1].n === 4);

    // A class instance lists its fields, not its methods, as in JavaScript.
    assert(sumXY(new Coords()) === 3);
}

function widest(r: Record<string, 1> | Record<string, number>): number[] {
    return Object.values(r);
}

function statusOf(r: Record<string, Status.Open> | Record<string, Status.Done>): Status[] {
    return Object.values(r);
}

function firstCreated(seen: Seen[] | Record<string, Seen>): string {
    return Object.values(seen)[0].created;
}

class Coords {
    x: number = 1;
    y: number = 2;
    norm(): number {
        return Math.sqrt(this.x * this.x + this.y * this.y);
    }
}
