// expect-error: binding `e` is already declared in this scope
function main(): void { try { throw new Error("x"); } catch(e) { const e = "x"; } }
