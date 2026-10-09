function f(x: number = 1 + 1): number { return x; }
function main(): void { assert(f() === 2, "expression default"); }
