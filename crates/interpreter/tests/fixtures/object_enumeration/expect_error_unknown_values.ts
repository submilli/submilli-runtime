// Where tsc falls back to `values(o: {}): any[]`, the values stay `unknown`:
// an interface has no implicit index signature, and unrelated value types
// leave tsc no candidate. So do values whose type a runtime check can't
// verify (class instances, interfaces with methods, recursive interfaces,
// type parameters), and an array a regex match array could widen into or bind
// to a type parameter, which would also list the match's `index`, `input` and
// `groups`.
// expect-error-count: 12
// expect-error: cannot read field `x` on `unknown`
// expect-error: cannot read field `y` on `unknown`
// expect-error: cannot read field `v` on `unknown`
// expect-error: expected `T[]`, got `unknown[]`
// expect-error: expected `E[]`, got `unknown[]`
// expect-error: expected `number`, got `unknown[]`
// expect-error: expected `string`, got `unknown[]`
// expect-error: expected `boolean`, got `unknown[]`
// expect-error: expected `null`, got `unknown[]`
// expect-error: expected `number`, got `(number | undefined)[]`
// expect-error: expected `number`, got `(number | string)[]`
// expect-error: expected `number`, got `[string, Seen][]`

interface Point {
    x: number;
    y: number;
}

interface Seen {
    created: string;
}

interface Runner {
    run(): number;
}

interface TreeNode {
    children: TreeNode[];
}

class Box {
    v: number = 1;
}

function valuesOf<T>(byName: Record<string, T>): T[] {
    return Object.values(byName);
}

// A match array bound to `E` would list `index`, `input` and `groups` too.
function elementsOf<E>(xs: E[]): E[] {
    return Object.values(xs);
}

function unrelated(either: Record<string, number> | Record<string, string>): number {
    return Object.values(either);
}

function main(): void {
    const p: Point = { x: 1, y: 2 };
    for (const v of Object.values(p)) console.log(v.x);
    for (const [k, v] of Object.entries(p)) console.log(k, v.y);
    const boxes: Record<string, Box> = { a: new Box() };
    for (const box of Object.values(boxes)) console.log(box.v);
    console.log(valuesOf({ a: 1 }).length, elementsOf([1]).length, unrelated({ a: 1 }));

    const runners: Record<string, Runner> = {};
    const s: string = Object.values(runners);
    const trees: Record<string, TreeNode> = {};
    const b: boolean = Object.values(trees);
    const parts: (string | undefined)[] = [];
    const n: null = Object.values(parts);

    // Exact types.
    const optional: { a?: number; b?: number } = {};
    const n1: number = Object.values(optional);
    const n2: number = Object.values({ a: 1, b: "x" });
    const n3: number = Object.entries({ a: { created: "" } } as Record<string, Seen>);
    console.log(s, b, n, n1, n2, n3);
}
