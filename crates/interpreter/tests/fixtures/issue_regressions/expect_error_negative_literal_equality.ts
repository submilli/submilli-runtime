// expect-error: expected
function f(x: 1): boolean { return x === -2; }
function main(): void { f(1); }
