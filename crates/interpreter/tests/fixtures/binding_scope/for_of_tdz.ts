// expect-error: cannot access `a` before its initialization
function main(): void { const a = [1]; for(const a of a) { console.log(a); } }
