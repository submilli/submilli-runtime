// expect-error: binding `a` is already declared in this scope
function f(a: number): number { const a = 2; return a; } function main(): void {}
