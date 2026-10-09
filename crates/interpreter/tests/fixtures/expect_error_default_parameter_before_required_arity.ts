// expect-error: argument
function f(a: number = 4, b: number): number { return a + b; }
function main(): void { f(); }
