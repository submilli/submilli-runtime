// expect-error: binding `a` is already declared in this scope
function main(): void { const a = 1; const a = 2; assert(a === 2); }
