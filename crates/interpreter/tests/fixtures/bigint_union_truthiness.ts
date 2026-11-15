function branch(g: bigint, n: number): string {
    if (g && n) { return "then"; }
    return "else";
}
function negate(g: bigint, n: number): boolean { return !(g && n); }
function bare(g: bigint): boolean { return !!g; }
function optional(g: bigint | undefined): boolean { return !!g; }
function literal(g: 0n | number): boolean { return !!g; }
function main(): void {
    assert(branch(7n, 1) === "then");
    assert(branch(7n, 0) === "else");
    assert(branch(0n, 1) === "else");
    assert(negate(0n, 1));
    assert(!bare(0n) && bare(-7n));
    assert(!optional(0n) && !optional(undefined) && optional(7n));
    assert(!literal(0n) && !literal(0) && literal(1));
}
