// expect-error: cannot access `x` before its initialization
function main(): void { const x: number[][] = [[7]]; for (const [x] of x) { console.log(x); } }
