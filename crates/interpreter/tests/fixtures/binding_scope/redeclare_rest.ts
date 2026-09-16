// expect-error: binding `rest` is already declared in this scope
function main(): void { const rest = 1; const { a, ...rest } = { a: 1, b: 2 }; }
