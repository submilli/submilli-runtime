// expect-error: binding `a` is already declared in this scope
function main(): void { let a = 1; let a = 2; }
