type A = { b?: B; x: number };
type B = { a?: A };
function chain(a: A): number | undefined { return a.b?.a?.x; }
function main(): void {
    assert(chain({ x: 1 }) === undefined);
    assert(chain({ x: 1, b: { a: { x: 2 } } }) === 2);
}
