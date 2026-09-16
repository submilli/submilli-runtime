// expect-error: expected 2 argument(s), got 1
function main(): number { const parseInt = (s: string, n: number): number => n; return parseInt("x"); }
