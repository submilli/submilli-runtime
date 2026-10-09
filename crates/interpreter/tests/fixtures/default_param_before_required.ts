function f(a: number = 4, b: number): number { return a + b; }
function main(): void { assert(f(undefined, 2) === 6, "default before required"); }
