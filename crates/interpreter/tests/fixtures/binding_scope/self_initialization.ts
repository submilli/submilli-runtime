// expect-error: cannot access `a` before its initialization
function main(): void { const a = 1; { const a = a + 1; } }
