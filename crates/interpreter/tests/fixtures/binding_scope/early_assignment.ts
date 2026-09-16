// expect-error: cannot access `a` before its initialization
function main(): void { let a = 1; { a = 2; let a = 3; } }
