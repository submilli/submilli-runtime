// expect-error: cannot access `a` before its initialization
function main(): string { const a = "O"; { const b = a; const a = "I"; return b + a; } }
